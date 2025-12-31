# frozen_string_literal: true

module Descent
  # Validates IR and collects warnings/errors.
  #
  # Run after IR building to catch issues before code generation.
  class Validator
    Warning = Data.define(:message, :location)
    Error   = Data.define(:message, :location)

    attr_reader :warnings, :errors

    def initialize(ir)
      @ir       = ir
      @warnings = []
      @errors   = []
    end

    def validate
      validate_types
      validate_functions
      validate_entry_point

      self
    end

    def valid? = @errors.empty?

    def report
      output = []

      @errors.each do |e|
        loc = e.location ? " (#{e.location})" : ''
        output << "ERROR#{loc}: #{e.message}"
      end

      @warnings.each do |w|
        loc = w.location ? " (#{w.location})" : ''
        output << "WARNING#{loc}: #{w.message}"
      end

      output.join("\n")
    end

    private

    def warn(message, location: nil) = @warnings << Warning.new(message:, location:)

    def error(message, location: nil) = @errors << Error.new(message:, location:)

    # ========== Type Validation ==========

    def validate_types
      seen_types = {}

      @ir.types.each do |type|
        loc = "L#{type.lineno}"

        # Check for duplicate type names
        if seen_types[type.name]
          error "Duplicate type declaration: #{type.name}",
                location: loc
        end
        seen_types[type.name] = type

        # Check for valid kind
        unless %i[bracket content internal].include?(type.kind)
          error "Unknown type kind '#{type.kind}' for #{type.name}",
                location: loc
        end
      end
    end

    # ========== Function Validation ==========

    def validate_functions
      seen_functions = {}

      @ir.functions.each do |func|
        loc = "L#{func.lineno}"

        # Check for duplicate function names
        if seen_functions[func.name]
          warn "Duplicate function definition: #{func.name}",
               location: loc
        end
        seen_functions[func.name] = func

        # Check for empty functions (no states)
        if func.states.empty?
          warn "Function '#{func.name}' has no states",
               location: loc
        end

        # Check return type exists
        if func.return_type && !type_exists?(func.return_type)
          warn "Return type '#{func.return_type}' not declared",
               location: loc
        end

        validate_function_states(func)
      end
    end

    def validate_function_states(func)
      func.states.each do |state|
        loc = "L#{state.lineno}"

        # Check for empty states (no cases)
        if state.cases.empty? && state.eof_handler.nil?
          warn "State '#{state.name}' in #{func.name} has no cases",
               location: loc
        end

        # NOTE: EOF handling is inferred per spec, so we don't warn about missing handlers
        # The code generator will infer appropriate EOF behavior based on:
        # - MARK status (emit content if marked)
        # - Return type (BRACKET types emit End, CONTENT types emit content)
        # - EXPECTS annotation (error if not satisfied)

        validate_state_cases(func, state)
      end
    end

    def validate_state_cases(func, state)
      loc = "L#{state.lineno}"

      state.cases.each do |kase|
        # Check for empty character matches
        if kase.chars&.empty? && !kase.default? && kase.special_class.nil?
          warn "Empty character match in #{func.name}:#{state.name}",
               location: loc
        end

        # Check for cases with no commands (this is actually often valid - just advance)
        # if kase.commands.empty?
        #   warn "Case with no commands in #{func.name}:#{state.name}",
        #        location: loc
        # end

        validate_commands(func, state, kase.commands, loc)
      end
    end

    def validate_commands(func, _state, commands, loc)
      commands.each do |cmd|
        case cmd.type
        when :call
          # Check if called function exists
          # IR builder stores name in :name field (not :value)
          func_name = cmd.args[:name] || cmd.args['name']
          unless func_name && function_exists?(func_name)
            warn "Call to undefined function '#{func_name}'",
                 location: loc
          end
        when :emit
          # Check if emitted type exists
          emitted = cmd.args[:value] || cmd.args['value']
          # Strip any suffix like "Start" or "End"
          base_type = emitted&.sub(/(Start|End|Anon)$/, '')
          unless type_exists?(base_type) || builtin_emit?(emitted)
            warn "Emit of undefined type '#{emitted}'",
                 location: loc
          end
        when :transition
          # Check if target state exists (if specified)
          target = cmd.args[:value] || cmd.args['value']
          next if target.nil? || target.empty? # Self-loop, valid

          if target.start_with?(':')
            # Target is :statename - validate state exists
            state_name = target.delete_prefix(':')
            unless state_exists_in_function?(func, state_name)
              warn "Transition to undefined state '#{target}'",
                   location: loc
            end
          else
            # Target doesn't start with : but isn't empty - probably malformed
            warn "Invalid transition target '#{target}' (should be :statename or empty)",
                 location: loc
          end
        end
      end
    end

    # ========== Entry Point Validation ==========

    def validate_entry_point
      return unless @ir.entry_point

      # Parse entry point: /function or /function:state
      entry = @ir.entry_point.delete_prefix('/')
      func_name, _state_name = entry.split(':')

      return if function_exists?(func_name)

      error "Entry point references undefined function '#{func_name}'",
            location: 'entry-point'
    end

    # ========== Helpers ==========

    def type_exists?(name)
      return false if name.nil?

      @ir.types.any? { |t| t.name.downcase == name.downcase }
    end

    def function_exists?(name)
      return false if name.nil?

      @ir.functions.any? { |f| f.name == name }
    end

    def state_exists_in_function?(func, state_name) = func.states.any? { |s| s.name == state_name }

    def has_eof_in_cases?(_state) = false

    def builtin_emit?(name) = %w[Error Warning].include?(name)
  end
end
