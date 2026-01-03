# frozen_string_literal: true

module Descent
  # Error raised when lexer encounters invalid syntax
  class LexerError < StandardError
    attr_reader :lineno, :source_file

    def initialize(message, lineno: nil, source_file: nil)
      @lineno      = lineno
      @source_file = source_file
      location     = [source_file, lineno].compact.join(':')
      super(location.empty? ? message : "#{location}: #{message}")
    end
  end

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
    #
    # Raises LexerError on unterminated quotes or brackets.
    def split_on_pipes(content)
      parts            = []
      current          = +''
      in_bracket       = false
      in_quote         = nil # nil, or the quote character (' or ")
      prev_char        = nil
      lineno           = 1
      quote_start_line = nil

      content.each_char do |c|
        lineno += 1 if c == "\n"

        case c
        when "'"
          current << c
          if in_quote == "'" && prev_char != '\\'
            in_quote = nil  # Close single quote (unless escaped)
          elsif in_quote.nil?
            in_quote = "'"  # Open single quote
            quote_start_line = lineno
          end
        when '"'
          current << c
          if in_quote == '"' && prev_char != '\\'
            in_quote = nil  # Close double quote (unless escaped)
          elsif in_quote.nil?
            in_quote = '"'  # Open double quote
            quote_start_line = lineno
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

      # Validate: no unterminated quotes or brackets
      if in_quote
        raise LexerError.new(
          "Unterminated #{in_quote == "'" ? 'single' : 'double'} quote - opened but never closed",
          lineno: quote_start_line,
          source_file: @source_file
        )
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
              if depth.zero?
                comment_start = i
                break
              end
            end
          end
          prev_char = c
        end
        comment_start ? "#{line[0...comment_start].rstrip}\n" : line
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

      while i < part.length && depth.positive?
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
            content << c if depth.positive? # Don't include final ]
          end
        else
          content << c
        end
        i += 1
      end

      [content, depth.zero? ? i : nil]
    end

    def parse_part(part, lineno)
      # Parse: TAG[ID] REST
      # TAG is everything up to [ or space
      # ID is inside []
      # REST is everything after
      #
      # Comments start with ; and go to end of line (or end of part)

      # Strip comments (but not semicolons inside brackets, parens, or quotes)
      # A comment starts with ; only if not inside [], (), or ''
      part = part.lines.map.with_index do |line, line_idx|
        # Find first ; that's not inside brackets, parens, or quotes
        bracket_depth = 0
        paren_depth = 0
        in_quote = false
        quote_start_col = nil
        comment_start = nil
        i = 0
        while i < line.length
          c = line[i]
          if in_quote
            in_quote = false if c == "'" && (i.zero? || line[i - 1] != '\\')
          else
            case c
            when "'"
              in_quote = true
              quote_start_col = i
            when '[' then bracket_depth += 1
            when ']' then bracket_depth -= 1
            when '(' then paren_depth += 1
            when ')' then paren_depth -= 1
            when ';'
              if bracket_depth.zero? && paren_depth.zero?
                comment_start = i
                break
              end
            end
          end
          i += 1
        end

        # Validate: unterminated quote within this part
        if in_quote
          raise LexerError.new(
            "Unterminated single quote at column #{quote_start_col + 1}",
            lineno: lineno + line_idx,
            source_file: @source_file
          )
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

      tag = case raw_tag
            when /^emit\(/i
              raw_tag
            when %r{^/\w+\(}
              # Function call - preserve case of arguments inside parens
              name = raw_tag[%r{^/(\w+)\(}, 1]
              args = raw_tag[/\(([^)]*)\)/, 1]
              "/#{name.downcase}(#{args})"
            when /^[A-Z]+(_[A-Z]+)*$/
              # SCREAMING_SNAKE_CASE - character class like LETTER, LABEL_CONT, DIGIT
              # Lowercase it so parser can handle it uniformly
              raw_tag.downcase
            when /^[A-Z]/
              # PascalCase - inline type emit, preserve case entirely
              raw_tag
            else
              raw_tag.downcase
            end

      # Extract ID from brackets, respecting quotes (so c[']'] works correctly)
      id, id_end_pos = extract_bracketed_id(part)

      # For function calls, strip the full call including parens
      after_tag = if raw_tag.match?(%r{^/\w+\(})
                    part.sub(%r{^/\w+\([^)]*\)}, '')
                  else
                    part.sub(/^(\.|[^ \[]+)/, '')
                  end
      rest = id_end_pos ? after_tag[(after_tag.index('[') + id.length + 2)..].to_s.strip : after_tag.strip

      # For parser name and similar, take only first word/line
      rest = rest.split("\n").first&.strip || '' if %w[parser entry-point].include?(tag)

      # Skip empty tags (artifacts of split)
      return nil if tag.empty? && id.empty? && rest.empty?

      Token.new(type: :part, tag:, id:, rest:, lineno:)
    end
  end
end
