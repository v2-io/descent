# frozen_string_literal: true

module Descent
  # Builds AST from token stream.
  #
  # Input: Array of Lexer::Token
  # Output: AST::Machine
  class Parser
    # Structural keywords that end a state, function, or keywords block
    STRUCTURAL = %w[function type state keywords].freeze

    # Keywords that start a new case within a state
    CASE_KEYWORDS = %w[c default eof if].freeze

    # Character class names (lowercase words that start cases, not commands)
    # ASCII classes: letter, digit, etc.
    # Unicode classes: xid_start, xid_cont, xlbl_start, xlbl_cont
    # Whitespace: ws, nl
    CHAR_CLASSES = %w[letter label_cont digit hex_digit ws nl xid_start xid_cont xlbl_start xlbl_cont].freeze

    # All tokens that can start a new case (used to know when to stop parsing current case)
    CASE_STARTERS = (STRUCTURAL + CASE_KEYWORDS + CHAR_CLASSES).freeze

    def initialize(tokens)
      @tokens = tokens
      @pos    = 0
    end

    # Detect if a token tag looks like a command (not a case starter).
    # Commands can start bare action cases.
    #
    # Commands include:
    # - Function calls: /word or /word(...)
    # - Arrows: -> or ->[...] or >>
    # - Uppercase commands: WORD or WORD(...) like MARK, TERM, PREPEND, EMIT
    # - Specific lowercase: return, err
    def command_like?(tag)
      return false if tag.nil?

      # Function call: /word
      return true if tag.start_with?('/')

      # Arrow commands: -> or >>
      return true if tag.start_with?('->') || tag.start_with?('>>')

      # Uppercase command: WORD or WORD(...)
      return true if tag.match?(/^[A-Z]/)

      # Specific lowercase commands (not character classes)
      base_tag = tag.downcase.split('(').first
      %w[return err mark term].include?(base_tag)
    end

    # Check if a token represents an inline command at function level.
    # This includes assignments like `result = 0` and commands like MARK.
    def inline_command_token?(token)
      tag  = token.tag
      rest = token.rest

      # Commands we already recognize
      return true if command_like?(tag)

      # Assignment: tag is variable name, rest starts with = or += or -=
      return true if rest&.match?(/^\s*[+-]?=/)

      false
    end

    def parse
      name        = nil
      entry_point = nil
      types       = []
      functions   = []
      keywords    = []

      while (token = current)
        case token.tag
        when 'parser'
          name = token.rest.strip.downcase
          advance
        when 'entry-point'
          entry_point = token.rest.strip
          advance
        when 'type'     then types << parse_type
        when 'function' then functions << parse_function
        when 'keywords' then keywords << parse_keywords
        else
          raise ParseError, "Line #{token.lineno}: Unknown top-level declaration '#{token.tag}'. " \
                            'Expected: parser, entry-point, type, function, or keywords'
        end
      end

      AST::Machine.new(name:, entry_point:, types:, functions:, keywords:)
    end

    private

    def current  = @tokens[@pos]
    def advance  = @pos += 1
    def peek     = @tokens[@pos + 1]

    def parse_type
      token = current
      name  = token.id
      # Take first word only (e.g., "BRACKET" from "BRACKET ; comment")
      kind  = token.rest.split.first&.upcase&.to_sym || :UNKNOWN
      advance

      AST::TypeDecl.new(name:, kind:, lineno: token.lineno)
    end

    # Parse keywords block for phf perfect hash lookup
    # Syntax: |keywords[name] :fallback /function(args)
    #           | keyword => EventType
    #           | keyword => EventType
    def parse_keywords
      token    = current
      name     = token.id
      rest     = token.rest
      lineno   = token.lineno
      advance

      # Parse fallback function from rest (e.g., ":fallback /bare_string" or "/bare_string")
      fallback = nil
      if rest =~ %r{:fallback\s+(/\w+(?:\([^)]*\))?)}
        fallback = ::Regexp.last_match(1)
      elsif rest =~ %r{^(/\w+(?:\([^)]*\))?)}
        fallback = ::Regexp.last_match(1)
      end

      mappings = []

      # Parse keyword mappings: | keyword => EventType
      while (t = current) && !STRUCTURAL.include?(t.tag) && !t.tag.start_with?('/')
        # Empty tag with rest containing "keyword => EventType"
        if t.tag == '' && t.rest.include?('=>')
          keyword, event_type = t.rest.split('=>', 2).map(&:strip)
          mappings << { keyword:, event_type: } if keyword && event_type
          advance
        # Tag is the keyword, rest contains "=> EventType"
        elsif t.rest =~ /^=>\s*(\w+)/
          keyword    = t.tag.strip
          event_type = ::Regexp.last_match(1)
          mappings << { keyword:, event_type: }
          advance
        else
          raise ParseError, "Line #{t.lineno}: Unknown keyword mapping format: '#{t.tag}' rest='#{t.rest}'"
        end
      end

      AST::Keywords.new(name:, fallback:, mappings:, lineno:)
    end

    def parse_function
      token       = current
      name, rtype = token.id.split(':')
      params      = parse_params(token.rest)
      lineno      = token.lineno
      advance

      states        = []
      eof_handler   = nil
      entry_actions = [] # Commands to execute on function entry (e.g., | result = 0)

      while (t = current) && !%w[function type keywords].include?(t.tag)
        case t.tag
        when 'state' then states << parse_state
        when 'eof'   then eof_handler = parse_eof_handler
        when 'if'    then entry_actions << parse_conditional # Function-level guard condition
        else
          # Check if this is an inline command (assignment, MARK, etc.)
          if inline_command_token?(t)
            entry_actions << parse_command(t)
            advance
          else
            raise ParseError, "Line #{t.lineno}: Unexpected token '#{t.tag}' inside function. " \
                              "Expected: state, eof, if, or inline command (like 'var = expr' or 'MARK')"
          end
        end
      end

      AST::Function.new(
        name: name.gsub('-', '_'),
        return_type: rtype,
        params:,
        states:,
        eof_handler:,
        entry_actions:,
        lineno:
      )
    end

    def parse_params(rest)
      return [] if rest.nil? || rest.empty?

      rest.scan(/:(\w+)/).flatten
    end

    def parse_state
      token    = current
      name     = token.id.gsub('-', '_').delete(':')
      lineno   = token.lineno
      advance

      cases       = []
      eof_handler = nil

      while (t = current) && !STRUCTURAL.include?(t.tag)
        case t.tag
        # Case keywords - specific case types
        when 'c'       then cases << parse_case(t.id)
        when 'default' then cases << parse_case(nil)
        when 'eof'     then eof_handler = parse_eof_handler
        when 'if'      then cases << parse_if_case
        else
          # Check for character class (lowercase word like 'letter', 'digit')
          if CHAR_CLASSES.include?(t.tag)
            cases << parse_case(t.tag.upcase)
          # Check for command-like tokens that start bare action cases
          elsif command_like?(t.tag)
            cases << parse_bare_action_case
          else
            raise ParseError, "Line #{t.lineno}: Unknown token in state: '#{t.tag}' (not a case starter or command)"
          end
        end
      end

      AST::State.new(name:, cases:, eof_handler:, lineno:)
    end

    def parse_case(chars_str)
      token   = current
      lineno  = token.lineno
      advance

      substate = nil
      commands = []

      while (t = current) && !CASE_STARTERS.include?(t.tag)
        case t.tag
        when '.'
          substate = t.rest.strip
        else
          commands << parse_command(t)
        end
        advance
      end

      AST::Case.new(
        chars: chars_str,
        substate:,
        commands:,
        lineno:
      )
    end

    # Parse a bare action case - one that starts with a command (like /function)
    # instead of a character match. Used for unconditional action states.
    def parse_bare_action_case
      token  = current
      lineno = token.lineno
      # Don't advance - the current token IS the first command

      substate = nil
      commands = []

      while (t = current) && !CASE_STARTERS.include?(t.tag)
        case t.tag
        when '.'
          substate = t.rest.strip
        else
          commands << parse_command(t)
        end
        advance
      end

      AST::Case.new(
        chars: nil,
        substate:,
        commands:,
        lineno:
      )
    end

    def parse_if_case
      token     = current
      lineno    = token.lineno
      condition = token.id
      advance

      commands      = []
      last_cmd_type = nil

      while (t = current) && !CASE_STARTERS.include?(t.tag)
        # After return, any command-like token starts a new case (bare action case).
        # The return is final - nothing should follow in the same case.
        break if last_cmd_type == :return && command_like?(t.tag)

        unless t.tag == '.'
          cmd = parse_command(t)
          commands << cmd
          last_cmd_type = cmd.type
        end
        advance
      end

      AST::Case.new(condition:, commands:, lineno:)
    end

    def parse_eof_handler
      token  = current
      lineno = token.lineno
      advance

      commands = []

      while (t = current) && !CASE_STARTERS.include?(t.tag)
        commands << parse_command(t) unless t.tag == '.'
        advance
      end

      AST::EOFHandler.new(commands:, lineno:)
    end

    def parse_command(token)
      # Determine command type from tag or content
      type, value = classify_command(token)
      AST::Command.new(type:, value:, lineno: token.lineno)
    end

    def classify_command(token)
      tag  = token.tag
      rest = token.rest

      case tag
      when ''
        # Inline command in rest
        parse_inline_command(rest)
      when '->'       then token.id.empty? ? [:advance, nil] : [:advance_to, token.id]
      when '>>'       then [:transition, rest.strip]
      when 'return'   then [:return, rest.strip]
      when 'err'      then [:error, rest.strip]
      when 'mark'     then [:mark, nil]
      when 'term'     then [:term, nil]
      when /^emit\(/i then [:emit, tag[/emit\(([^)]+)\)/i, 1]]
      when %r{^/\w}   then [:call, tag[1..] + (rest.empty? ? '' : "(#{rest})")]
      when /^TERM\((-?\d+)\)$/i       then [:term, ::Regexp.last_match(1).to_i]
      when /^TERM$/i                  then [:term, 0]
      when /^MARK$/i                  then [:mark, nil]
      when /^KEYWORDS\((\w+)\)$/i     then [:keywords_lookup, ::Regexp.last_match(1)]
      when /^PREPEND\(([^)]*)\)$/i
        content = ::Regexp.last_match(1).strip
        if content.empty?
          [:noop, nil]
        elsif content.start_with?(':')
          [:prepend_param, content[1..]] # Strip leading colon
        else
          [:prepend, content]
        end
      when /^([A-Z]\w*)\(USE_MARK\)$/  then [:inline_emit_mark, ::Regexp.last_match(1)]
      when /^([A-Z]\w*)\(([^)]+)\)$/   then [:inline_emit_literal,
                                             { type: ::Regexp.last_match(1), literal: ::Regexp.last_match(2) }]
      when /^([A-Z]\w*)$/              then [:inline_emit_bare, ::Regexp.last_match(1)]
      else
        # Check if tag + rest forms an assignment (e.g., tag="depth", rest="= 1")
        full_cmd = "#{tag} #{rest}".strip
        parse_inline_command(full_cmd)
      end
    end

    def parse_inline_command(cmd)
      cmd = cmd.strip
      return [:noop, nil] if cmd.empty?

      case cmd
      when /^MARK\b/i             then [:mark, nil]
      when /^TERM\((-?\d+)\)/i    then [:term, ::Regexp.last_match(1).to_i]
      when /^TERM\b/i             then [:term, 0]
      when /^KEYWORDS\((\w+)\)/i  then [:keywords_lookup, ::Regexp.last_match(1)]
      when /^PREPEND\(([^)]*)\)/i
        content = ::Regexp.last_match(1).strip
        if content.empty?
          [:noop, nil]
        elsif content.start_with?(':')
          [:prepend_param, content[1..]] # Strip leading colon
        else
          [:prepend, content]
        end
      when /^return\b\s*(.*)$/i   then [:return, ::Regexp.last_match(1).strip]
      when /^->\s*$/              then [:advance, nil]
      when /^->\s*\[([^\]]+)\]$/  then [:advance_to, ::Regexp.last_match(1)]
      when /^emit\(([^)]+)\)/i    then [:emit, ::Regexp.last_match(1)]
      when /^CALL:(\w+)/i         then [:call_method, ::Regexp.last_match(1)]
      when /^SCAN\(([^)]+)\)/i    then [:scan, ::Regexp.last_match(1)]
      when %r{^/(\w+)} then [:call, ::Regexp.last_match(1)]
      when /^(\w+)\s*\+=\s*(.+)$/ then [:add_assign, { var: ::Regexp.last_match(1), expr: ::Regexp.last_match(2) }]
      when /^(\w+)\s*-=\s*(.+)$/  then [:sub_assign, { var: ::Regexp.last_match(1), expr: ::Regexp.last_match(2) }]
      when /^(\w+)\s*=\s*(.+)$/   then [:assign, { var: ::Regexp.last_match(1), expr: ::Regexp.last_match(2) }]
      when /^([A-Z]\w*)\(USE_MARK\)$/  then [:inline_emit_mark, ::Regexp.last_match(1)]
      when /^([A-Z]\w*)\(([^)]+)\)$/   then [:inline_emit_literal,
                                             { type: ::Regexp.last_match(1), literal: ::Regexp.last_match(2) }]
      when /^([A-Z]\w*)$/              then [:inline_emit_bare, ::Regexp.last_match(1)]
      else
        raise ParseError, "Unrecognized command: '#{cmd}'. " \
                          'Expected: MARK, TERM, PREPEND, return, ->, /call, assignment, or TypeName'
      end
    end

    def parse_conditional
      token   = current
      lineno  = token.lineno
      clauses = []

      current_condition = token.id
      current_commands  = []
      advance

      loop do
        t = current
        break unless t

        case t.tag
        when 'elsif'
          clauses << AST::Clause.new(condition: current_condition, commands: current_commands)
          current_condition = t.id
          current_commands  = []
          advance
        when 'else'
          clauses << AST::Clause.new(condition: current_condition, commands: current_commands)
          current_condition = nil
          current_commands  = []
          advance
        when 'endif'
          clauses << AST::Clause.new(condition: current_condition, commands: current_commands)
          advance
          break
        when 'function', 'type', 'state', 'c', 'default', 'eof'
          # Implicit endif
          clauses << AST::Clause.new(condition: current_condition, commands: current_commands)
          break
        else
          current_commands << parse_command(t)
          advance
        end
      end

      AST::Conditional.new(clauses:, lineno:)
    end
  end
end
