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
      def initialize(name:, entry_point:, types: [], functions: [])
        super
      end
    end

    # Resolved type information
    TypeInfo = Data.define(:name, :kind, :emits_start, :emits_end) do
      # kind: :bracket (emits Start/End), :content (emits on return), :internal (no emit)
      def initialize(name:, kind:, emits_start: false, emits_end: false)
        super
      end

      def bracket?  = kind == :bracket
      def content?  = kind == :content
      def internal? = kind == :internal
    end

    # Function with resolved semantics
    Function = Data.define(:name, :return_type, :params, :locals, :states, :eof_handler, :emits_events) do
      def initialize(name:, return_type: nil, params: [], locals: {}, states: [], eof_handler: nil, emits_events: false)
        super
      end
    end

    # State with inferred optimizations
    State = Data.define(:name, :cases, :eof_handler, :scan_chars, :is_self_looping) do
      # scan_chars: Array of chars for SIMD memchr scan, or nil if not applicable
      # is_self_looping: true if has default case that loops back to self
      def initialize(name:, cases: [], eof_handler: nil, scan_chars: nil, is_self_looping: false)
        super
      end

      def scannable? = !scan_chars.nil? && !scan_chars.empty?
    end

    # Case with resolved actions
    Case = Data.define(:chars, :special_class, :substate, :commands) do
      # chars: Array of literal chars to match, or nil for default
      # special_class: Symbol like :letter, :label_cont for special matchers
      def initialize(chars: nil, special_class: nil, substate: nil, commands: [])
        super
      end

      def default? = chars.nil? && special_class.nil?
    end

    # Resolved command
    Command = Data.define(:type, :args) do
      # type: :mark, :term, :advance, :emit, :call, :assign, :return, :transition, etc.
      # args: Hash of type-specific arguments
      def initialize(type:, args: {})
        super
      end
    end

    # Conditional with resolved conditions
    Conditional = Data.define(:clauses)

    Clause = Data.define(:condition, :commands)
  end
end
