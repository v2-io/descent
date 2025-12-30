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
      lineno = 1

      # Track line numbers as we go
      line_offsets = [0]
      @content.each_char.with_index do |c, i|
        line_offsets << (i + 1) if c == "\n"
      end

      # Strip comments BEFORE splitting on pipes to avoid corrupted parsing
      # when a comment contains a pipe character
      content_without_comments = strip_comments(@content)

      # Split on pipes, tracking position
      # Skip comment-only lines (starting with ;)
      # Use bracket-aware split to handle |c[|] correctly
      parts       = []
      current_pos = 0

      split_on_pipes(content_without_comments).each do |part|
        next if part.strip.empty?

        # Find line number for this part (use original content for position tracking)
        found_pos = content_without_comments.index(part, current_pos) || current_pos
        line = line_offsets.bsearch_index { |off| off > found_pos } || line_offsets.size
        lineno = line

        parts << [part.rstrip, lineno]
        current_pos = found_pos + part.length
      end

      parts.each do |part, line|
        token = parse_part(part, line)
        tokens << token if token
      end

      tokens
    end

    # Split content on pipes, but not on pipes inside bracket-delimited IDs
    # This correctly handles cases like |c[|] where the pipe is a literal
    # Also handles |c[LETTER'[.?!] where [ inside is literal, not a delimiter
    def split_on_pipes(content)
      parts = []
      current = +''
      in_bracket = false

      content.each_char do |c|
        case c
        when '['
          # Only first [ opens the bracket context - nested [ are literal
          in_bracket = true unless in_bracket
          current << c
        when ']'
          # ] always closes the bracket context (only one level)
          current << c
          in_bracket = false
        when '|'
          if in_bracket
            current << c
          else
            parts << current unless current.empty?
            current = +''
          end
        else
          current << c
        end
      end
      parts << current unless current.empty?
      parts
    end

    # Strip comments from content, preserving semicolons inside brackets
    def strip_comments(content)
      content.lines.map do |line|
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
        comment_start ? line[0...comment_start].rstrip + "\n" : line
      end.join
    end

    private

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
      id      = part[/\[([^\]]*)\]/, 1] || ''
      # For function calls, strip the full call including parens
      rest = if raw_tag.match?(%r{^/\w+\(})
               part.sub(%r{^/\w+\([^)]*\)}, '').sub(/\[[^\]]*\]/, '').strip
             else
               part.sub(/^(\.|[^ \[]+)/, '').sub(/\[[^\]]*\]/, '').strip
             end

      # For parser name and similar, take only first word/line
      rest = rest.split("\n").first&.strip || '' if %w[parser entry-point].include?(tag)

      # Skip empty tags (artifacts of split)
      return nil if tag.empty? && id.empty? && rest.empty?

      Token.new(type: :part, tag:, id:, rest:, lineno:)
    end
  end
end
