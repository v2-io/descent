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

    # Convert snake_case or lowercase to PascalCase.
    # Examples: "identity" -> "Identity", "after_name" -> "AfterName"
    def pascalcase(str)
      return '' if str.nil?

      str.to_s.split(/[_\s-]/).map(&:capitalize).join
    end

    # Transform DSL expressions to Rust.
    # - COL -> self.col()
    # - PREV -> self.prev()
    # - /function(args) -> self.parse_function(args, on_event)
    # - /function -> self.parse_function(on_event)
    # - Character literals: ' ' -> b' ', '\t' -> b'\t' (only if not already a byte literal)
    # - Escape sequences: <R> -> b']', <RB> -> b'}', <P> -> b'|', etc.
    # - Parameter references: :param -> param
    def rust_expr(str)
      return '' if str.nil?

      # First, transform call arguments (handles :param, <R>, etc.)
      # This catches standalone expressions like "<R>" or ":close"
      result = transform_call_args(str.to_s)

      result
        .gsub(/\bCOL\b/, 'self.col()')
        .gsub(/\bPREV\b/, 'self.prev()')
        .gsub(%r{/(\w+)\(([^)]*)\)}) do
          func = ::Regexp.last_match(1)
          args = transform_call_args(::Regexp.last_match(2))
          "self.parse_#{func}(#{args}, on_event)"
        end
        .gsub(%r{/(\w+)}) { "self.parse_#{::Regexp.last_match(1)}(on_event)" }
        .gsub(/(?<!b)'(\\.|.)'/, "b'\\1'")  # Convert char literals to byte literals (only if not already b'...')
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
        when '<R>'                then "b']'"
        when '<RB>'               then "b'}'"
        when '<L>'                then "b'['"
        when '<LB>'               then "b'{'"
        when '<P>'                then "b'|'"
        when '<BS>'               then "b'\\\\'"
        when '<RP>'               then "b')'"  # Right paren
        when '<LP>'               then "b'('"  # Left paren
        when '"'                  then "b'\"'" # Bare double quote
        when "'"                  then "b'\\''" # Bare single quote (escaped)
        when /^\d+$/              then arg # numeric literals
        when /^-?\d+$/            then arg # negative numbers
        when /^'(.)'$/            then "b'#{::Regexp.last_match(1)}'" # char literal
        when /^"(.)"$/            then "b'#{::Regexp.last_match(1)}'" # quoted char
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
        .gsub(/^(\/\/.*)\n(use |pub |impl )/, "\\1\n\n\\2")   # Blank before use/pub/impl
        .gsub(/(\})\n([ \t]*(?:\/\/|#\[|pub |fn ))/, "\\1\n\n\\2")  # Blank after } before new item
    end

    private

    def build_context
      {
        'parser'             => @ir.name,
        'entry_point'        => @ir.entry_point,
        'types'              => @ir.types.map { |t| type_to_hash(t) },
        'functions'          => @ir.functions.map { |f| function_to_hash(f) },
        'custom_error_codes' => @ir.custom_error_codes,
        'trace'              => @trace
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
      {
        'name'                   => func.name,
        'return_type'            => func.return_type,
        'params'                 => func.params,
        'param_types'            => func.param_types.transform_keys(&:to_s).transform_values(&:to_s),
        'locals'                 => func.locals.transform_keys(&:to_s),
        'states'                 => func.states.map { |s| state_to_hash(s) },
        'eof_handler'            => func.eof_handler&.map { |c| command_to_hash(c) } || [],
        'emits_events'           => func.emits_events,
        'expects_char'           => func.expects_char,
        'emits_content_on_close' => func.emits_content_on_close
      }
    end

    def state_to_hash(state)
      {
        'name'            => state.name,
        'cases'           => state.cases.map { |c| case_to_hash(c) },
        'eof_handler'     => state.eof_handler&.map { |c| command_to_hash(c) } || [],
        'scan_chars'       => state.scan_chars,
        'scannable'        => state.scannable?,
        'is_self_looping'  => state.is_self_looping,
        'has_default'      => state.has_default,
        'is_unconditional' => state.is_unconditional
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
        'is_default'     => kase.default?
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
