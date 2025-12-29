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

      IR::Parser.new(
        name:        @ast.name,
        entry_point: @ast.entry_point,
        types:,
        functions:
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

      IR::Function.new(
        name:         func.name,
        return_type:  func.return_type,
        params:       func.params,
        locals:,
        states:,
        eof_handler:  func.eof_handler,
        emits_events:,
        lineno:       func.lineno
      )
    end

    def build_state(state)
      cases           = state.cases.map { |c| build_case(c) }
      scan_chars      = infer_scan_chars(state, cases)
      is_self_looping = cases.any? { |c| c.default? && has_self_transition?(c) }

      IR::State.new(
        name:            state.name,
        cases:,
        eof_handler:     state.eof_handler,
        scan_chars:,
        is_self_looping:,
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
      args = case cmd.type
             when :assign, :add_assign, :sub_assign then cmd.value.is_a?(Hash) ? cmd.value : {}
             when :advance_to, :scan then { value: process_escapes(cmd.value) }
             when :emit, :call, :call_method, :transition, :error then { value: cmd.value }
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
         .gsub('\\n', "\n")
         .gsub('\\t', "\t")
    end

    # Parse character specification into literal chars or special class
    def parse_chars(chars_str)
      return [nil, nil] if chars_str.nil?

      # Check for special named classes (all uppercase)
      return [nil, chars_str.downcase.to_sym] if chars_str.match?(/^[A-Z_]+$/)

      # Parse literal characters with escapes
      [process_escapes(chars_str).chars, nil]
    end

    # Infer SCAN optimization: if a state has a self-looping default case,
    # the explicit character cases become SCAN targets
    def infer_scan_chars(_state, cases)
      default_case = cases.find(&:default?)
      return nil unless default_case
      return nil unless has_self_transition?(default_case)

      # Collect all explicit characters from non-default cases
      explicit_chars = cases
                       .reject(&:default?)
                       .flat_map { |c| c.chars || [] }
                       .uniq

      return nil if explicit_chars.empty?
      return nil if explicit_chars.size > 4 # memchr only supports up to 4

      explicit_chars
    end

    def has_self_transition?(kase)
      kase.commands.any? do |cmd|
        next false unless cmd.type == :transition

        val = cmd.args[:value] || cmd.args['value']
        val.nil? || val.empty?
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
        case cmd.type
        when :assign, :add_assign, :sub_assign
          if cmd.value.is_a?(Hash) && cmd.value[:var]
            locals[cmd.value[:var]] ||= :i32 # Default type
          end
        when AST::Conditional
          cmd.clauses&.each do |clause|
            collect_locals_from_commands(clause.commands, locals)
          end
        end
      end
    end
  end
end
