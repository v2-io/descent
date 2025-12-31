# frozen_string_literal: true

module Descent
  # Intermediate Representation - semantic model after analysis.
  #
  # IR nodes are enriched with inferred information:
  # - Resolved types
  # - SCAN optimization characters (inferred from state structure)
  # - EOF handling requirements
  # - Local variable declarations with types
  module IR
    # Top-level parser definition
    Parser = Data.define(:name, :entry_point, :types, :functions, :keywords, :custom_error_codes) do
      def initialize(name:, entry_point:, types: [], functions: [], keywords: [], custom_error_codes: []) = super
    end

    # Keywords for perfect hash (phf) lookup
    # Generates static phf::Map for O(1) keyword matching
    Keywords = Data.define(:name, :fallback_func, :fallback_args, :mappings, :lineno) do
      # name: identifier (e.g., "bare" generates BARE_KEYWORDS)
      # fallback_func: function to call when no keyword matches
      # fallback_args: arguments to pass to fallback
      # mappings: Array of {keyword: "string", event_type: "TypeName"}
      def initialize(name:, fallback_func: nil, fallback_args: nil, mappings: [], lineno: 0) = super
    end

    # Resolved type information
    TypeInfo = Data.define(:name, :kind, :emits_start, :emits_end, :lineno) do
      # kind: :bracket (emits Start/End), :content (emits on return), :internal (no emit)
      def initialize(name:, kind:, emits_start: false, emits_end: false, lineno: 0) = super

      def bracket?  = kind == :bracket
      def content?  = kind == :content
      def internal? = kind == :internal
    end

    # Function with resolved semantics
    Function = Data.define(:name, :return_type, :params, :param_types, :locals, :states, :eof_handler, :emits_events,
                           :expects_char, :emits_content_on_close, :prepend_values, :lineno) do
      # params: Array of parameter names
      # param_types: Hash mapping param name -> :byte or :i32 (inferred from usage)
      # expects_char: Single char that must be seen to return (inferred from return cases)
      # emits_content_on_close: Whether TERM appears before return (emit content on unclosed EOF)
      # prepend_values: Hash mapping param name -> Array of byte values that could be passed (for PREPEND)
      def initialize(name:, return_type: nil, params: [], param_types: {}, locals: {}, states: [], eof_handler: nil,
                     emits_events: false, expects_char: nil, emits_content_on_close: false, prepend_values: {}, lineno: 0)
        super
      end

      def expects_closer? = !expects_char.nil?
    end

    # State with inferred optimizations
    State = Data.define(:name, :cases, :eof_handler, :scan_chars, :is_self_looping, :has_default, :is_unconditional, :lineno) do
      # scan_chars: Array of chars for SIMD memchr scan, or nil if not applicable
      # is_self_looping: true if has default case that loops back to self
      # has_default: true if state has a default case (no chars, no condition)
      # is_unconditional: true if first case has no char match (bare action case)
      def initialize(name:, cases: [], eof_handler: nil, scan_chars: nil, is_self_looping: false,
                     has_default: false, is_unconditional: false, lineno: 0) = super

      def scannable? = !scan_chars.nil? && !scan_chars.empty?
    end

    # Case with resolved actions
    Case = Data.define(:chars, :special_class, :param_ref, :condition, :substate, :commands) do
      # chars: Array of literal chars to match, or nil for default
      # special_class: Symbol like :letter, :label_cont for special matchers
      # param_ref: Parameter name to match against (for |c[:param]|), or nil
      # condition: String condition for if-cases, or nil
      def initialize(chars: nil, special_class: nil, param_ref: nil, condition: nil, substate: nil, commands: []) = super

      def default?     = chars.nil? && special_class.nil? && param_ref.nil? && condition.nil?
      def conditional? = !condition.nil?
    end

    # Resolved command
    Command = Data.define(:type, :args) do
      # type: :mark, :term, :advance, :emit, :call, :assign, :return, :transition, etc.
      # args: Hash of type-specific arguments
      def initialize(type:, args: {}) = super
    end

    # Conditional with resolved conditions
    Conditional = Data.define(:clauses)

    Clause = Data.define(:condition, :commands)
  end
end
