# frozen_string_literal: true

module Descent
  # Tokenizes .desc files (pipe-delimited UDON format).
  #
  # Input: Raw file content
  # Output: Array of Token structs
  class Lexer
    Token = Data.define(:type, :tag, :id, :rest, :lineno)

    def initialize(content, source_file: '(string)')
      @content     = content
      @source_file = source_file
    end

    def tokenize
      tokens = []

      # Strip comments BEFORE splitting on pipes to avoid corrupted parsing
      # when a comment contains a pipe character.
      # Note: strip_comments preserves line structure (each original line maps
      # to exactly one stripped line), so we can count newlines to get line numbers.
      content_without_comments = strip_comments(@content)

      # Split on pipes, tracking position
      # Skip comment-only lines (starting with ;)
      # Use bracket-aware split to handle |c[|] correctly
      parts       = []
      current_pos = 0

      split_on_pipes(content_without_comments).each do |part|
        next if part.strip.empty?

        # Find position in stripped content
        found_pos = content_without_comments.index(part, current_pos) || current_pos

        # Count newlines before this position to get line number (1-indexed).
        # This works because strip_comments preserves line count - each original
        # line becomes exactly one line in stripped content.
        lineno = content_without_comments[0...found_pos].count("\n") + 1

        parts << [part.rstrip, lineno]
        current_pos = found_pos + part.length
      end

      parts.each do |part, line|
        token = parse_part(part, line)
        tokens << token if token
      end

      tokens
    end

    # Split content on pipes, but not on pipes inside bracket-delimited IDs or quotes
    # This correctly handles cases like |c[|] where the pipe is a literal
    # Also handles |c[LETTER'[.?!] where [ inside is literal, not a delimiter
    # Also handles '/sameline_text(elem_col, '|')' where | is in quotes
    def split_on_pipes(content)
      parts      = []
      current    = +''
      in_bracket = false
      in_quote   = nil # nil, or the quote character (' or ")
      prev_char  = nil

      content.each_char do |c|
        case c
        when "'"
          current << c
          if in_quote == "'" && prev_char != '\\'
            in_quote = nil  # Close single quote (unless escaped)
          elsif in_quote.nil?
            in_quote = "'"  # Open single quote
          end
        when '"'
          current << c
          if in_quote == '"' && prev_char != '\\'
            in_quote = nil  # Close double quote (unless escaped)
          elsif in_quote.nil?
            in_quote = '"'  # Open double quote
          end
        when '['
          # Only first [ opens the bracket context - nested [ are literal
          in_bracket ||= true unless in_quote
          current << c
        when ']'
          # ] always closes the bracket context (only one level)
          current << c
          in_bracket = false unless in_quote
        when '|'
          if in_bracket || in_quote
            current << c
          else
            parts << current unless current.empty?
            current = +''
          end
        else
          current << c
        end
        prev_char = c
      end
      parts << current unless current.empty?
      parts
    end

    # Strip comments from content, preserving semicolons inside brackets and quotes
    def strip_comments(content)
      content.lines.map do |line|
        depth = 0
        in_quote = nil
        prev_char = nil
        comment_start = nil

        line.each_char.with_index do |c, i|
          # Track quote state (respecting escapes)
          if c == "'" && prev_char != '\\' && in_quote != '"'
            in_quote = in_quote == "'" ? nil : "'"
          elsif c == '"' && prev_char != '\\' && in_quote != "'"
            in_quote = in_quote == '"' ? nil : '"'
          elsif !in_quote
            case c
            when '[' then depth += 1
            when ']' then depth -= 1
            when ';'
              if depth == 0
                comment_start = i
                break
              end
            end
          end
          prev_char = c
        end
        comment_start ? line[0...comment_start].rstrip + "\n" : line
      end.join
    end

    private

    # Extract the content inside [...] from a part string, respecting single quotes.
    # Returns [content, end_position] or ['', nil] if no brackets found.
    # This handles cases like c[']'] where ] inside quotes shouldn't close the bracket.
    # Only single quotes are quote delimiters in c[...] - double quotes are literals.
    def extract_bracketed_id(part)
      start_pos = part.index('[')
      return ['', nil] unless start_pos

      i        = start_pos + 1
      depth    = 1
      in_quote = false
      content  = +''

      while i < part.length && depth > 0
        c = part[i]

        case c
        when "'"
          content << c
          in_quote = !in_quote
        when '['
          content << c
          depth += 1 unless in_quote
        when ']'
          if in_quote
            content << c
          else
            depth -= 1
            content << c if depth > 0 # Don't include final ]
          end
        else
          content << c
        end
        i += 1
      end

      [content, depth == 0 ? i : nil]
    end

    def parse_part(part, lineno)
      # Parse: TAG[ID] REST
      # TAG is everything up to [ or space
      # ID is inside []
      # REST is everything after
      #
      # Comments start with ; and go to end of line (or end of part)

      # Strip comments (but not semicolons inside brackets)
      # A comment starts with ; only if not inside []
      part = part.lines.map do |line|
        # Find first ; that's not inside brackets
        depth = 0
        comment_start = nil
        line.each_char.with_index do |c, i|
          case c
          when '[' then depth += 1
          when ']' then depth -= 1
          when ';'
            if depth == 0
              comment_start = i
              break
            end
          end
        end
        comment_start ? line[0...comment_start].rstrip : line.rstrip
      end.join("\n").strip

      # Extract tag - downcase unless it's emit(), function call, or inline type emit
      # For function calls with parens, capture the full call including arguments
      raw_tag = if part.match?(%r{^/\w+\(})
                  # Function call - capture up to and including closing paren
                  part[%r{^/\w+\([^)]*\)}] || part[/^[^ \[]+/]
                else
                  part[/^(\.|[^ \[]+)/]
                end&.strip || ''

      tag = if raw_tag.match?(/^emit\(/i)
              raw_tag
            elsif raw_tag.match?(%r{^/\w+\(})
              # Function call - preserve case of arguments inside parens
              name = raw_tag[%r{^/(\w+)\(}, 1]
              args = raw_tag[/\(([^)]*)\)/, 1]
              "/#{name.downcase}(#{args})"
            elsif raw_tag.match?(/^[A-Z]+(_[A-Z]+)*$/)
              # SCREAMING_SNAKE_CASE - character class like LETTER, LABEL_CONT, DIGIT
              # Lowercase it so parser can handle it uniformly
              raw_tag.downcase
            elsif raw_tag.match?(/^[A-Z]/)
              # PascalCase - inline type emit, preserve case entirely
              raw_tag
            else
              raw_tag.downcase
            end

      # Extract ID from brackets, respecting quotes (so c[']'] works correctly)
      id, id_end_pos = extract_bracketed_id(part)

      # For function calls, strip the full call including parens
      rest = if raw_tag.match?(%r{^/\w+\(})
               after_tag = part.sub(%r{^/\w+\([^)]*\)}, '')
               id_end_pos ? after_tag[(after_tag.index('[') + id.length + 2)..].to_s.strip : after_tag.strip
             else
               after_tag = part.sub(/^(\.|[^ \[]+)/, '')
               id_end_pos ? after_tag[(after_tag.index('[') + id.length + 2)..].to_s.strip : after_tag.strip
             end

      # For parser name and similar, take only first word/line
      rest = rest.split("\n").first&.strip || '' if %w[parser entry-point].include?(tag)

      # Skip empty tags (artifacts of split)
      return nil if tag.empty? && id.empty? && rest.empty?

      Token.new(type: :part, tag:, id:, rest:, lineno:)
    end
  end
end
