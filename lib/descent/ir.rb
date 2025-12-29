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
    Parser = Data.define(:name, :entry_point, :types, :functions) do
      def initialize(name:, entry_point:, types: [], functions: []) = super
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
    Function = Data.define(:name, :return_type, :params, :locals, :states, :eof_handler, :emits_events,
                           :expects_char, :emits_content_on_close, :lineno) do
      # expects_char: Single char that must be seen to return (inferred from return cases)
      # emits_content_on_close: Whether TERM appears before return (emit content on unclosed EOF)
      def initialize(name:, return_type: nil, params: [], locals: {}, states: [], eof_handler: nil,
                     emits_events: false, expects_char: nil, emits_content_on_close: false, lineno: 0)
        super
      end

      def expects_closer? = !expects_char.nil?
    end

    # State with inferred optimizations
    State = Data.define(:name, :cases, :eof_handler, :scan_chars, :is_self_looping, :lineno) do
      # scan_chars: Array of chars for SIMD memchr scan, or nil if not applicable
      # is_self_looping: true if has default case that loops back to self
      def initialize(name:, cases: [], eof_handler: nil, scan_chars: nil, is_self_looping: false, lineno: 0) = super

      def scannable? = !scan_chars.nil? && !scan_chars.empty?
    end

    # Case with resolved actions
    Case = Data.define(:chars, :special_class, :condition, :substate, :commands) do
      # chars: Array of literal chars to match, or nil for default
      # special_class: Symbol like :letter, :label_cont for special matchers
      # condition: String condition for if-cases, or nil
      def initialize(chars: nil, special_class: nil, condition: nil, substate: nil, commands: []) = super

      def default?     = chars.nil? && special_class.nil? && condition.nil?
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
