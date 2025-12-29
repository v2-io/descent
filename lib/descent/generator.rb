# frozen_string_literal: true

require "liquid"

module Descent
  # Renders IR to target language code using Liquid templates.
  #
  # All target-specific logic lives in templates, not here.
  class Generator
    TEMPLATE_DIR = File.expand_path("templates", __dir__)

    def initialize(ir, target:, trace: false, **options)
      @ir      = ir
      @target  = target
      @trace   = trace
      @options = options
    end

    def generate
      template_path = File.join(TEMPLATE_DIR, @target.to_s, "parser.liquid")

      unless File.exist?(template_path)
        raise Error, "No template for target: #{@target} (looked in #{template_path})"
      end

      template = Liquid::Template.parse(File.read(template_path))

      template.render(
        build_context,
        strict_variables: true,
        strict_filters:   true
      )
    end

    private

    def build_context
      {
        "parser"      => @ir.name,
        "entry_point" => @ir.entry_point,
        "types"       => @ir.types.map { |t| type_to_hash(t) },
        "functions"   => @ir.functions.map { |f| function_to_hash(f) },
        "trace"       => @trace
      }
    end

    def type_to_hash(type)
      {
        "name"        => type.name,
        "kind"        => type.kind.to_s,
        "emits_start" => type.emits_start,
        "emits_end"   => type.emits_end
      }
    end

    def function_to_hash(func)
      {
        "name"         => func.name,
        "return_type"  => func.return_type,
        "params"       => func.params,
        "locals"       => func.locals.transform_keys(&:to_s),
        "states"       => func.states.map { |s| state_to_hash(s) },
        "eof_handler"  => func.eof_handler,
        "emits_events" => func.emits_events
      }
    end

    def state_to_hash(state)
      {
        "name"            => state.name,
        "cases"           => state.cases.map { |c| case_to_hash(c) },
        "eof_handler"     => state.eof_handler,
        "scan_chars"      => state.scan_chars,
        "scannable"       => state.scannable?,
        "is_self_looping" => state.is_self_looping
      }
    end

    def case_to_hash(kase)
      {
        "chars"         => kase.chars,
        "special_class" => kase.special_class&.to_s,
        "substate"      => kase.substate,
        "commands"      => kase.commands.map { |c| command_to_hash(c) },
        "is_default"    => kase.default?
      }
    end

    def command_to_hash(cmd)
      {
        "type" => cmd.type.to_s,
        "args" => cmd.args.transform_keys(&:to_s)
      }
    end
  end
end
