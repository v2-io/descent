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
    # - Other transformations can be added here
    def rust_expr(str)
      return '' if str.nil?

      str.to_s.gsub(/\bCOL\b/, 'self.col()')
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

      # Post-process: collapse excessive whitespace (3+ newlines -> 2)
      result.gsub(/\n{3,}/, "\n\n")
    end

    private

    def build_context
      {
        'parser'      => @ir.name,
        'entry_point' => @ir.entry_point,
        'types'       => @ir.types.map { |t| type_to_hash(t) },
        'functions'   => @ir.functions.map { |f| function_to_hash(f) },
        'trace'       => @trace
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
        'locals'                 => func.locals.transform_keys(&:to_s),
        'states'                 => func.states.map { |s| state_to_hash(s) },
        'eof_handler'            => func.eof_handler,
        'emits_events'           => func.emits_events,
        'expects_char'           => func.expects_char,
        'emits_content_on_close' => func.emits_content_on_close
      }
    end

    def state_to_hash(state)
      {
        'name'            => state.name,
        'cases'           => state.cases.map { |c| case_to_hash(c) },
        'eof_handler'     => state.eof_handler,
        'scan_chars'      => state.scan_chars,
        'scannable'       => state.scannable?,
        'is_self_looping' => state.is_self_looping
      }
    end

    def case_to_hash(kase)
      {
        'chars'          => kase.chars,
        'special_class'  => kase.special_class&.to_s,
        'condition'      => kase.condition,
        'is_conditional' => kase.conditional?,
        'substate'       => kase.substate,
        'commands'       => kase.commands.map { |c| command_to_hash(c) },
        'is_default'     => kase.default?
      }
    end

    def command_to_hash(cmd) = { 'type' => cmd.type.to_s, 'args' => cmd.args.transform_keys(&:to_s) }
  end
end
