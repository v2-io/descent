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

      # Split on pipes, tracking position
      # Skip comment-only lines (starting with ;)
      parts       = []
      current_pos = 0

      @content.split('|').each do |part|
        next if part.strip.empty?
        next if part.strip.start_with?(';')

        # Find line number for this part
        found_pos = @content.index(part, current_pos) || current_pos
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
      raw_tag = part[/^(\.|[^ \[]+)/]&.strip || ''
      tag = if raw_tag.match?(/^emit\(/i)
              raw_tag
            elsif raw_tag.match?(%r{^/\w+\(})
              # Function call - preserve case of arguments inside parens
              name = raw_tag[%r{^/(\w+)\(}, 1]
              args = raw_tag[/\(([^)]*)\)/, 1]
              "/#{name.downcase}(#{args})"
            elsif raw_tag.match?(/^[A-Z]/)
              # PascalCase - inline type emit, preserve case entirely
              raw_tag
            else
              raw_tag.downcase
            end
      id      = part[/\[([^\]]*)\]/, 1] || ''
      rest = part
             .sub(/^(\.|[^ \[]+)/, '')
             .sub(/\[[^\]]*\]/, '')
             .strip

      # For parser name and similar, take only first word/line
      rest = rest.split("\n").first&.strip || '' if %w[parser entry-point].include?(tag)

      # Skip empty tags (artifacts of split)
      return nil if tag.empty? && id.empty? && rest.empty?

      Token.new(type: :part, tag:, id:, rest:, lineno:)
    end
  end
end
