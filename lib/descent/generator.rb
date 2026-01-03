# frozen_string_literal: true

require 'liquid'

module Descent
  # Custom Liquid filters for code generation.
  module LiquidFilters
    # Convert a character to Rust byte literal format.
    # Examples: "\n" -> "b'\\n'", "|" -> "b'|'", " " -> "b' '"
    def escape_rust_char(char)
      return 'b\'?\'' if char.nil?

      escaped = case char
                when "\n" then '\\n'
                when "\t" then '\\t'
                when "\r" then '\\r'
                when '\\' then '\\\\'
                when "'" then "\\'"
                else char
                end
      "b'#{escaped}'"
    end

    # Convert snake_case, camelCase, or preserve PascalCase.
    # Examples: "identity" -> "Identity", "after_name" -> "AfterName",
    #           "UnclosedInterpolation" -> "UnclosedInterpolation"
    def pascalcase(str)
      return '' if str.nil?

      # Split on delimiters AND on case transitions (lowercase followed by uppercase)
      # This preserves existing PascalCase while converting snake_case and camelCase
      str.to_s.split(/[_\s-]|(?<=[a-z])(?=[A-Z])/).map(&:capitalize).join
    end

    # Transform DSL expressions to Rust.
    # - /function(args) -> self.parse_function(transformed_args, on_event)
    # - /function -> self.parse_function(on_event)
    # - COL -> self.col()
    # - LINE -> self.line as i32
    # - PREV -> self.prev()
    # - Character literals: ' ' -> b' ', '\t' -> b'\t' (only if not already a byte literal)
    # - Escape sequences: <R> -> b']', <RB> -> b'}', <P> -> b'|', etc.
    # - Parameter references: :param -> param
    def rust_expr(str)
      return '' if str.nil?

      result = str.to_s

      # IMPORTANT: Process function calls FIRST, before COL/LINE/PREV expansion.
      # Otherwise /element(COL) becomes /element(self.col()) and the regex
      # [^)]* breaks on the ) inside self.col().
      result = result.gsub(%r{/(\w+)\(([^)]*)\)}) do
        func = ::Regexp.last_match(1)
        # Transform args: :param, <R>, COL, etc.
        args = transform_call_args(::Regexp.last_match(2))
        args = expand_special_vars(args)
        "self.parse_#{func}(#{args}, on_event)"
      end
      result = result.gsub(%r{/(\w+)}) { "self.parse_#{::Regexp.last_match(1)}(on_event)" }

      # Now expand special variables in the rest of the expression
      result = expand_special_vars(result)

      # Transform standalone args (handles :param, <R>, etc. outside function calls)
      result = transform_call_args(result)

      result
        .gsub(/(?<!b)'(\\.|.)'/, "b'\\1'")  # Convert char literals to byte literals (only if not already b'...')
        # Escape sequences embedded in expressions (not just standalone args)
        .gsub('<P>', "b'|'")
        .gsub('<R>', "b']'")
        .gsub('<L>', "b'['")
        .gsub('<RB>', "b'}'")
        .gsub('<LB>', "b'{'")
        .gsub('<RP>', "b')'")
        .gsub('<LP>', "b'('")
        .gsub('<BS>', "b'\\\\'")
        .gsub('<SQ>', "b'\\''")
        .gsub('<DQ>', "b'\"'")
        .gsub('<NL>', "b'\\n'")
        .gsub('<WS>', "b' '")
    end

    # Expand special variables: COL, LINE, PREV, :param
    def expand_special_vars(str)
      str
        .gsub(/\bCOL\b/, 'self.col()')
        .gsub(/\bLINE\b/, 'self.line as i32')
        .gsub(/\bPREV\b/, 'self.prev()')
        .gsub(/:([a-z_]\w*)/i) { ::Regexp.last_match(1) }  # :param -> param
    end

    # Transform function call arguments.
    # - :param -> param (parameter references)
    # - <R> -> b']', <RB> -> b'}', <RP> -> b')', etc. (escape sequences to byte literals)
    # - Bare quotes: " -> b'"', ' -> b'\''
    def transform_call_args(args)
      args.split(',').map do |arg|
        arg = arg.strip
        case arg
        when /^:(\w+)$/           then ::Regexp.last_match(1) # :param -> param
        when '<>'                 then 'b""' # Empty byte slice
        when '<R>'                then "b']'"
        when '<RB>'               then "b'}'"
        when '<L>'                then "b'['"
        when '<LB>'               then "b'{'"
        when '<P>'                then "b'|'"
        when '<BS>'               then "b'\\\\'"
        when '<RP>'               then "b')'"  # Right paren
        when '<LP>'               then "b'('"  # Left paren
        when '<SQ>'               then "b'\\''" # Single quote
        when '<DQ>'               then "b'\"'" # Double quote
        when '"'                  then "b'\"'" # Bare double quote
        when "'"                  then "b'\\''" # Bare single quote (escaped)
        when /^\d+$/              then arg # numeric literals
        when /^-?\d+$/            then arg # negative numbers
        when /^'(.)'$/            then "b'#{::Regexp.last_match(1)}'" # char literal
        when /^"(.)"$/            then "b'#{::Regexp.last_match(1)}'" # quoted char
        when %r{^[!;:#*\-_<>/\\@$%^&+=?,.]$} then "b'#{arg}'" # Single punctuation → byte literal
        else arg # pass through (variables, expressions)
        end
      end.join(', ')
    end
  end

  # Custom file system for Liquid partials.
  class TemplateFileSystem
    def initialize(base_path) = @base_path = base_path

    def read_template_file(template_path)
      # Liquid looks for partials with underscore prefix
      full_path = File.join(@base_path, "_#{template_path}.liquid")
      raise Liquid::FileSystemError, "No such template: #{full_path}" unless File.exist?(full_path)

      File.read(full_path)
    end
  end

  # Renders IR to target language code using Liquid templates.
  #
  # All target-specific logic lives in templates, not here.
  class Generator
    TEMPLATE_DIR = File.expand_path('templates', __dir__)

    def initialize(ir, target:, trace: false, **options)
      @ir      = ir
      @target  = target
      @trace   = trace
      @options = options
    end

    def generate
      template_dir  = File.join(TEMPLATE_DIR, @target.to_s)
      template_path = File.join(template_dir, 'parser.liquid')

      raise Error, "No template for target: #{@target} (looked in #{template_path})" unless File.exist?(template_path)

      # Build Liquid environment with filters and file system
      env = Liquid::Environment.build do |e|
        e.register_filter(LiquidFilters)
        e.file_system = TemplateFileSystem.new(template_dir)
      end

      template = Liquid::Template.parse(File.read(template_path), environment: env)

      result = template.render(
        build_context,
        strict_variables: false, # Partials may not have all variables
        strict_filters:   true
      )

      # Post-process: clean up whitespace from Liquid template
      result
        .gsub(/^[ \t]+$/, '')                                 # Remove whitespace-only lines
        .gsub(/\n{2,}/, "\n")                                 # Collapse all blank lines
        .gsub(%r{^(//.*)\n(use |pub |impl )}, "\\1\n\n\\2")   # Blank before use/pub/impl
        .gsub(%r{(\})\n([ \t]*(?://|#\[|pub |fn ))}, "\\1\n\n\\2")  # Blank after } before new item
    end

    private

    # Unicode character classes that require the unicode-xid crate
    UNICODE_CLASSES = %w[xid_start xid_cont xlbl_start xlbl_cont].freeze

    def build_context
      functions_data = @ir.functions.map { |f| function_to_hash(f) }
      usage          = analyze_helper_usage(functions_data)
      {
        'parser'             => @ir.name,
        'entry_point'        => @ir.entry_point,
        'types'              => @ir.types.map { |t| type_to_hash(t) },
        'functions'          => functions_data,
        'keywords'           => @ir.keywords.map { |k| keywords_to_hash(k) },
        'custom_error_codes' => @ir.custom_error_codes,
        'trace'              => @trace,
        'uses_unicode'       => uses_unicode_classes?(functions_data),
        # Helper usage flags - only emit helpers that are actually used
        'uses_col'           => usage[:col],
        'uses_prev'          => usage[:prev],
        'uses_set_term'      => usage[:set_term],
        'uses_span'          => usage[:span],
        'uses_letter'        => usage[:letter],
        'uses_label_cont'    => usage[:label_cont],
        'uses_digit'         => usage[:digit],
        'uses_hex_digit'     => usage[:hex_digit],
        'uses_ws'            => usage[:ws],
        'uses_nl'            => usage[:nl],
        'max_scan_arity'     => usage[:max_scan_arity]
      }
    end

    # Analyze which helper methods are actually used by the generated code.
    # Returns a hash of usage flags that the template uses for conditional emission.
    def analyze_helper_usage(functions_data)
      usage = {
        col: false, prev: false, set_term: false, span: false,
        letter: false, label_cont: false, digit: false, hex_digit: false,
        ws: false, nl: false, max_scan_arity: 0
      }

      functions_data.each do |func|
        # Check conditions for COL/PREV usage
        check_expressions_in_function(func, usage)

        # Check special classes used in cases
        func['states'].each do |state|
          # Track max scan arity
          if state['scannable'] && state['scan_chars']
            usage[:max_scan_arity] = [usage[:max_scan_arity], state['scan_chars'].size].max
          end

          state['cases'].each do |kase|
            check_special_class(kase['special_class'], usage)
          end
        end
      end

      # span() is used for bracket types and errors (always needed if we have types)
      usage[:span] = true

      usage
    end

    # Check expressions in conditions/commands for COL/PREV usage
    def check_expressions_in_function(func, usage)
      all_commands = collect_all_commands(func)

      all_commands.each do |cmd|
        args = cmd['args'] || {}

        # Check condition expressions (from if cases and conditionals)
        check_expression(args['condition'], usage)

        # Check call arguments for COL/PREV
        check_expression(args['call_args'], usage) if cmd['type'] == 'call'

        # Check assignment expressions
        check_expression(args['expr'], usage) if %w[assign add_assign sub_assign].include?(cmd['type'])

        # Check for set_term usage (any TERM command uses set_term)
        usage[:set_term] = true if cmd['type'] == 'term'

        # Check for advance_to - track scan arity for explicit ->[chars]
        if cmd['type'] == 'advance_to' && args['value']
          arity = args['value'].length
          usage[:max_scan_arity] = [usage[:max_scan_arity], arity].max
        end
      end

      # Check case conditions
      func['states'].each do |state|
        state['cases'].each do |kase|
          check_expression(kase['condition'], usage)
        end
      end
    end

    # Collect all commands from a function (including nested in conditionals)
    def collect_all_commands(func)
      commands = []

      # Entry actions
      commands.concat(func['entry_actions'] || [])

      # EOF handler
      commands.concat(func['eof_handler'] || [])

      # State commands
      func['states'].each do |state|
        commands.concat(state['eof_handler'] || [])
        state['cases'].each do |kase|
          kase['commands'].each do |cmd|
            commands << cmd
            # Recurse into conditional clauses
            if cmd['type'] == 'conditional' && cmd.dig('args', 'clauses')
              cmd['args']['clauses'].each do |clause|
                commands.concat(clause['commands'] || [])
              end
            end
          end
        end
      end

      commands
    end

    def check_expression(expr, usage)
      return unless expr.is_a?(String)

      usage[:col]  = true if expr.match?(/\bCOL\b/)
      usage[:prev] = true if expr.match?(/\bPREV\b/)
    end

    def check_special_class(special_class, usage)
      return unless special_class

      case special_class.to_s
      when 'letter'     then usage[:letter] = true
      when 'label_cont' then usage[:label_cont] = true
      when 'digit'      then usage[:digit] = true
      when 'hex_digit'  then usage[:hex_digit] = true
      when 'ws'         then usage[:ws] = true
      when 'nl'         then usage[:nl] = true
      end
    end

    # Check if any function uses Unicode character classes
    def uses_unicode_classes?(functions_data)
      functions_data.any? do |func|
        func['states'].any? do |state|
          state['cases'].any? do |kase|
            special_class = kase['special_class']
            special_class && UNICODE_CLASSES.include?(special_class)
          end
        end
      end
    end

    def keywords_to_hash(kw)
      {
        'name'          => kw.name,
        'const_name'    => "#{kw.name.upcase}_KEYWORDS",
        'fallback_func' => kw.fallback_func,
        'fallback_args' => kw.fallback_args,
        'mappings'      => kw.mappings.map do |m|
          {
            'keyword'    => m[:keyword],
            'event_type' => m[:event_type]
          }
        end
      }
    end

    def type_to_hash(type)
      {
        'name'        => type.name,
        'kind'        => type.kind.to_s,
        'emits_start' => type.emits_start,
        'emits_end'   => type.emits_end
      }
    end

    def function_to_hash(func)
      # Extract initial values from entry_actions for locals
      # This allows template to initialize locals directly instead of "= 0" then assignment
      local_init_values = extract_local_init_values(func.entry_actions || [])

      # Filter out pure assignments from entry_actions (they become initializers)
      # Keep conditionals and non-assignment commands
      filtered_entry_actions = (func.entry_actions || []).reject do |cmd|
        cmd.type == :assign && local_init_values.key?(cmd.args[:var])
      end

      {
        'name'                   => func.name,
        'return_type'            => func.return_type,
        'params'                 => func.params,
        'param_types'            => func.param_types.transform_keys(&:to_s).transform_values(&:to_s),
        'locals'                 => func.locals.transform_keys(&:to_s),
        'local_init_values'      => local_init_values,
        'states'                 => func.states.map { |s| state_to_hash(s) },
        'eof_handler'            => func.eof_handler&.map { |c| command_to_hash(c) } || [],
        'entry_actions'          => filtered_entry_actions.map { |c| command_to_hash(c) },
        'emits_events'           => func.emits_events,
        'expects_char'           => func.expects_char,
        'emits_content_on_close' => func.emits_content_on_close,
        'prepend_values'         => func.prepend_values.transform_keys(&:to_s),
        'lineno'                 => func.lineno
      }
    end

    # Extract initial values for locals from entry_actions assignments
    def extract_local_init_values(entry_actions)
      init_values = {}
      entry_actions.each do |cmd|
        next unless cmd.type == :assign

        var  = cmd.args[:var]
        expr = cmd.args[:expr]
        # Only use simple literals as initializers
        init_values[var] = expr if var && expr&.match?(/^-?\d+$/)
      end
      init_values
    end

    def state_to_hash(state)
      {
        'name'              => state.name,
        'cases'             => state.cases.map { |c| case_to_hash(c) },
        'eof_handler'       => state.eof_handler&.map { |c| command_to_hash(c) } || [],
        'scan_chars'        => state.scan_chars,
        'scannable'         => state.scannable?,
        'is_self_looping'   => state.is_self_looping,
        'has_default'       => state.has_default,
        'is_unconditional'  => state.is_unconditional,
        'newline_injected'  => state.newline_injected,
        'lineno'            => state.lineno
      }
    end

    def case_to_hash(kase)
      {
        'chars'          => kase.chars,
        'special_class'  => kase.special_class&.to_s,
        'param_ref'      => kase.param_ref,
        'condition'      => kase.condition,
        'is_conditional' => kase.conditional?,
        'substate'       => kase.substate,
        'commands'       => kase.commands.map { |c| command_to_hash(c) },
        'is_default'     => kase.default?,
        'lineno'         => kase.lineno
      }
    end

    def command_to_hash(cmd)
      args = cmd.args.transform_keys(&:to_s)

      # Recursively convert nested commands in conditionals
      if cmd.type == :conditional && args['clauses']
        args['clauses'] = args['clauses'].map do |clause|
          {
            'condition' => clause['condition'],
            'commands'  => clause['commands'].map { |c| command_to_hash(c) }
          }
        end
      end

      { 'type' => cmd.type.to_s, 'args' => args }
    end
  end
end
