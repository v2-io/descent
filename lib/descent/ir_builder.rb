# frozen_string_literal: true

module Descent
  # Transforms AST into IR with semantic analysis.
  #
  # Responsibilities:
  # - Resolve type references
  # - Infer SCAN optimization characters from state structure
  # - Infer EOF handling requirements
  # - Collect local variable declarations
  # - Validate consistency
  class IRBuilder
    def initialize(ast) = @ast = ast

    def build
      types     = build_types(@ast.types)
      functions = @ast.functions.map { |f| build_function(f, types) }

      # Collect custom error codes from /error(code) calls
      custom_error_codes = collect_custom_error_codes(functions)

      IR::Parser.new(
        name:        @ast.name,
        entry_point: @ast.entry_point,
        types:,
        functions:,
        custom_error_codes:
      )
    end

    private

    def build_types(type_decls)
      type_decls.map do |t|
        emits_start = t.kind == :BRACKET
        emits_end   = t.kind == :BRACKET

        IR::TypeInfo.new(
          name:        t.name,
          kind:        t.kind.downcase.to_sym,
          emits_start:,
          emits_end:,
          lineno:      t.lineno
        )
      end
    end

    def build_function(func, types)
      return_type_info = types.find { |t| t.name == func.return_type }
      emits_events     = return_type_info&.bracket? || return_type_info&.content?

      locals = infer_locals(func)
      states = func.states.map { |s| build_state(s) }

      # Infer expected closing delimiter from return cases
      expects_char, emits_content_on_close = infer_expects(states)

      IR::Function.new(
        name:                   func.name,
        return_type:            func.return_type,
        params:                 func.params,
        locals:,
        states:,
        eof_handler:            func.eof_handler,
        emits_events:,
        expects_char:,
        emits_content_on_close:,
        lineno:                 func.lineno
      )
    end

    def build_state(state)
      cases           = state.cases.map { |c| build_case(c) }
      scan_chars      = infer_scan_chars(state, cases)
      is_self_looping = cases.any? { |c| c.default? && has_self_transition?(c) }

      # Check if state has a default case (no chars, no condition, no special_class)
      has_default = cases.any?(&:default?)

      # Check if first case is unconditional (bare action - no char match)
      # This means the state just executes actions without matching any character
      first_case = cases.first
      is_unconditional = first_case && first_case.chars.nil? && first_case.special_class.nil? && first_case.condition.nil?

      IR::State.new(
        name:            state.name,
        cases:,
        eof_handler:     state.eof_handler,
        scan_chars:,
        is_self_looping:,
        has_default:,
        is_unconditional:,
        lineno:          state.lineno
      )
    end

    def build_case(kase)
      chars, special_class = parse_chars(kase.chars)
      commands = kase.commands.map { |c| build_command(c) }

      IR::Case.new(
        chars:,
        special_class:,
        condition:     kase.condition,
        substate:      kase.substate,
        commands:
      )
    end

    def build_command(cmd)
      # Handle AST::Conditional specially
      if cmd.is_a?(AST::Conditional)
        return IR::Command.new(
          type: :conditional,
          args: {
            clauses: cmd.clauses&.map do |c|
              {
                'condition' => c.condition,
                'commands' => c.commands.map { |cc| build_command(cc) }
              }
            end
          }
        )
      end

      args = case cmd.type
             when :assign, :add_assign, :sub_assign then cmd.value.is_a?(Hash) ? cmd.value : {}
             when :advance_to, :scan then { value: process_escapes(cmd.value) }
             when :emit, :call, :call_method, :transition, :error then { value: cmd.value }
             when :inline_emit_bare, :inline_emit_mark then { type: cmd.value }
             when :inline_emit_literal then cmd.value.is_a?(Hash) ? cmd.value : {}
             when :term then { offset: cmd.value || 0 }
             when :prepend then { literal: process_escapes(cmd.value) }
             when :return then parse_return_value(cmd.value)
             else
               {}
             end

      IR::Command.new(type: cmd.type, args:)
    end

    # Process character escapes in a string
    def process_escapes(str)
      return str if str.nil?

      str.gsub('<L>', '[')
         .gsub('<R>', ']')
         .gsub('<LB>', '{')
         .gsub('<RB>', '}')
         .gsub('<P>', '|')
         .gsub('<BS>', '\\')
         .gsub('\\\\', '\\')  # \\ -> \ (escaped backslash)
         .gsub('\\n', "\n")
         .gsub('\\t', "\t")
    end

    # Parse character specification into literal chars and/or special class
    # Returns [chars_array, special_class_symbol]
    # Examples:
    #   "abc"           -> [["a", "b", "c"], nil]
    #   "LETTER"        -> [nil, :letter]
    #   "LETTER'[.?!"   -> [["'", "[", ".", "?", "!"], :letter]
    def parse_chars(chars_str)
      return [nil, nil] if chars_str.nil?

      # Check for pure special named class (SCREAMING_CASE: uppercase words separated by underscores)
      # Examples: LETTER, LABEL_CONT, HEX_DIGIT
      return [nil, chars_str.downcase.to_sym] if chars_str.match?(/^[A-Z]+(?:_[A-Z]+)*$/)

      # Check for combined: CLASS followed by literal chars (e.g., LETTER'[.?!)
      # Class portion is SCREAMING_CASE, literals start with non-uppercase
      if (match = chars_str.match(/^([A-Z]+(?:_[A-Z]+)*)(.+)$/))
        class_name = match[1].downcase.to_sym
        literal_chars = process_escapes(match[2]).chars
        return [literal_chars, class_name]
      end

      # Parse literal characters with escapes
      [process_escapes(chars_str).chars, nil]
    end

    # Parse return value specification
    # Returns hash with :emit_type, :emit_mode, :literal, :return_value
    # Examples:
    #   nil or ""        -> {} (default behavior)
    #   "TypeName"       -> { emit_type: "TypeName", emit_mode: :bare }
    #   "TypeName(USE_MARK)" -> { emit_type: "TypeName", emit_mode: :mark }
    #   "TypeName(lit)"  -> { emit_type: "TypeName", emit_mode: :literal, literal: "lit" }
    #   "varname"        -> { return_value: "varname" } (for INTERNAL types returning a value)
    def parse_return_value(value)
      return {} if value.nil? || value.empty?

      case value
      when /^([A-Z]\w*)\(USE_MARK\)$/
        { emit_type: ::Regexp.last_match(1), emit_mode: :mark }
      when /^([A-Z]\w*)\(([^)]+)\)$/
        { emit_type: ::Regexp.last_match(1), emit_mode: :literal, literal: process_escapes(::Regexp.last_match(2)) }
      when /^([A-Z]\w*)$/
        { emit_type: ::Regexp.last_match(1), emit_mode: :bare }
      when /^[a-z_]\w*$/
        # Variable name - for INTERNAL types returning a computed value
        { return_value: value }
      else
        {} # Unknown format, use default
      end
    end

    # Infer SCAN optimization: if a state has a simple self-looping default case
    # (only advance + transition, no side effects), the explicit character cases
    # become SCAN targets.
    def infer_scan_chars(_state, cases)
      default_case = cases.find(&:default?)
      return nil unless default_case
      return nil unless simple_self_loop?(default_case)

      # Collect all explicit characters from non-default cases
      explicit_chars = cases
                       .reject(&:default?)
                       .reject(&:conditional?) # Skip conditional cases
                       .flat_map { |c| c.chars || [] }
                       .uniq

      return nil if explicit_chars.empty?
      # Support up to 6 chars via chained memchr calls (memchr3 + memchr3)
      # Beyond 6, the overhead of chaining outweighs the benefit
      return nil if explicit_chars.size > 6

      explicit_chars
    end

    # Check if a case is a simple self-loop: only advance and/or transition (no calls, emits, etc.)
    # This is the stricter check for SCAN optimization.
    def simple_self_loop?(kase)
      has_self_transition = false

      kase.commands.each do |cmd|
        case cmd.type
        when :advance
          # OK - just advancing
        when :transition
          val = cmd.args[:value] || cmd.args['value']
          has_self_transition = true if val.nil? || val.empty?
        else
          # Any other command (call, emit, mark, term, etc.) means not a simple loop
          return false
        end
      end

      has_self_transition
    end

    # Check if a case has any self-transition (used for is_self_looping metadata)
    def has_self_transition?(kase)
      kase.commands.any? do |cmd|
        next false unless cmd.type == :transition

        val = cmd.args[:value] || cmd.args['value']
        val.nil? || val.empty?
      end
    end

    # Infer expected closing delimiter from return cases.
    # If ALL return cases match the same single character, that's the expected closer.
    # Also check if TERM appears before return (emits_content_on_close).
    def infer_expects(states)
      return_cases = []

      # Collect all cases that contain a return command
      states.each do |state|
        state.cases.each do |kase|
          return_cases << kase if kase.commands.any? { |cmd| cmd.type == :return }
        end
      end

      # No returns found - no expected closer
      return [nil, false] if return_cases.empty?

      # Check if all return cases match the same single character
      # (ignore conditional cases for now - they still match on a char)
      char_matches = return_cases.filter_map do |kase|
        # Must have exactly one character match (not default, not char class)
        next nil if kase.default?
        next nil if kase.special_class
        next nil if kase.chars.nil? || kase.chars.length != 1

        kase.chars.first
      end

      # If not all return cases have single-char matches, no expected closer
      return [nil, false] if char_matches.length != return_cases.length

      # If not all the same character, no expected closer
      return [nil, false] if char_matches.uniq.length != 1

      expects_char = char_matches.first

      # Check if any return case has TERM before return
      emits_content = return_cases.any? do |kase|
        kase.commands.any? { |cmd| cmd.type == :term }
      end

      [expects_char, emits_content]
    end

    # Collect custom error codes from /error(code) calls across all functions
    def collect_custom_error_codes(functions)
      codes = Set.new

      functions.each do |func|
        func.states.each do |state|
          state.cases.each do |kase|
            collect_error_codes_from_commands(kase.commands, codes)
          end
        end
      end

      codes.to_a.sort
    end

    def collect_error_codes_from_commands(commands, codes)
      commands.each do |cmd|
        case cmd.type
        when :error
          # /error(code) - collect the code
          code = cmd.args[:value] || cmd.args['value']
          codes << code if code && !code.empty?
        when :call
          # /error(code) is also parsed as a call sometimes
          value = cmd.args[:value] || cmd.args['value']
          if value&.start_with?('error(')
            code = value[/error\(([^)]+)\)/, 1]
            codes << code if code && !code.empty?
          end
        when :conditional
          # Recurse into conditional clauses
          cmd.args[:clauses]&.each do |clause|
            collect_error_codes_from_commands(clause['commands'] || [], codes)
          end
        end
      end
    end

    # Infer local variables from assignments in function
    def infer_locals(func)
      locals = {}

      func.states.each do |state|
        state.cases.each do |kase|
          collect_locals_from_commands(kase.commands, locals)
        end
      end

      locals
    end

    def collect_locals_from_commands(commands, locals)
      commands.each do |cmd|
        if cmd.is_a?(AST::Conditional)
          cmd.clauses&.each do |clause|
            collect_locals_from_commands(clause.commands, locals)
          end
        elsif cmd.respond_to?(:type)
          case cmd.type
          when :assign, :add_assign, :sub_assign
            if cmd.value.is_a?(Hash) && cmd.value[:var]
              locals[cmd.value[:var]] ||= :i32 # Default type
            end
          end
        end
      end
    end
  end
end
