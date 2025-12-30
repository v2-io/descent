# frozen_string_literal: true

module Descent
  # Builds AST from token stream.
  #
  # Input: Array of Lexer::Token
  # Output: AST::Machine
  class Parser
    def initialize(tokens)
      @tokens = tokens
      @pos    = 0
    end

    def parse
      name        = nil
      entry_point = nil
      types       = []
      functions   = []

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
        else
          advance # Skip unknown top-level tokens
        end
      end

      AST::Machine.new(name:, entry_point:, types:, functions:)
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

    def parse_function
      token       = current
      name, rtype = token.id.split(':')
      params      = parse_params(token.rest)
      lineno      = token.lineno
      advance

      states      = []
      eof_handler = nil

      while (t = current) && !%w[function type].include?(t.tag)
        case t.tag
        when 'state' then states << parse_state
        when 'eof'   then eof_handler = parse_eof_handler
        else
          advance # Skip inline commands at function level for now
        end
      end

      AST::Function.new(
        name:        name.gsub('-', '_'),
        return_type: rtype,
        params:,
        states:,
        eof_handler:,
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

      # Keywords that should NOT be treated as character classes
      keywords = %w[return err emit mark term]

      while (t = current) && !%w[function type state].include?(t.tag)
        case t.tag
        when 'c'       then cases << parse_case(t.id)
        when 'default' then cases << parse_case(nil)
        when 'eof'     then eof_handler = parse_eof_handler
        when 'if'      then cases << parse_if_case
        when '>>'
          # Bare >> is an else branch (default case with just transition)
          # Often follows |if[cond] |return as the else case
          cases << AST::Case.new(
            chars:    nil,
            substate: nil,
            commands: [parse_command(t)],
            lineno:   t.lineno
          )
          advance
        when /^[a-z_]+$/
          if keywords.include?(t.tag)
            advance # Skip - these are commands, not cases
          else
            cases << parse_case(t.tag.upcase) # Character class: letter, label_cont, etc.
          end
        else
          advance
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

      # Stop on any tag that could start a new case in parse_state
      # This includes: function, type, state, c, default, eof, if, and character classes
      case_starters = %w[function type state c default eof if letter label_cont digit hex_digit]

      while (t = current) && !case_starters.include?(t.tag)
        case t.tag
        when '.'
          substate = t.rest.strip
          advance
        else
          commands << parse_command(t)
          advance
        end
      end

      AST::Case.new(
        chars:    chars_str,
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

      commands = []
      # Case starters - anything that would start a new case or end the state
      case_starters = %w[function type state c default eof if letter label_cont]
      last_cmd_type = nil

      while (t = current) && !case_starters.include?(t.tag)
        # If previous command was return and current is >>, the >> is an else branch
        # (a new default case), not part of this case's transition
        break if last_cmd_type == :return && t.tag == '>>'

        if t.tag == '.'
          advance # Skip substate marker
        else
          cmd = parse_command(t)
          commands << cmd
          last_cmd_type = cmd.type
          advance
        end
      end

      AST::Case.new(condition:, commands:, lineno:)
    end

    def parse_eof_handler
      token  = current
      lineno = token.lineno
      advance

      commands = []

      while (t = current) && !%w[function type state c default eof].include?(t.tag)
        if t.tag == '.'
          advance # Skip substate marker
        else
          commands << parse_command(t)
          advance
        end
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
      when /^PREPEND\(([^)]+)\)$/i    then [:prepend, ::Regexp.last_match(1)]
      when /^([A-Z]\w*)\(USE_MARK\)$/  then [:inline_emit_mark, ::Regexp.last_match(1)]
      when /^([A-Z]\w*)\(([^)]+)\)$/   then [:inline_emit_literal, { type: ::Regexp.last_match(1), literal: ::Regexp.last_match(2) }]
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
      when /^PREPEND\(([^)]+)\)/i then [:prepend, ::Regexp.last_match(1)]
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
      when /^([A-Z]\w*)\(([^)]+)\)$/   then [:inline_emit_literal, { type: ::Regexp.last_match(1), literal: ::Regexp.last_match(2) }]
      when /^([A-Z]\w*)$/              then [:inline_emit_bare, ::Regexp.last_match(1)]
      else                             [:raw, cmd]
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
