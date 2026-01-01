# frozen_string_literal: true

module Descent
  # Abstract Syntax Tree nodes - pure data, no behavior.
  #
  # Uses Ruby 3.2+ Data class for immutable value objects.
  # These represent the direct parse result before semantic analysis.
  module AST
    # Top-level machine definition
    Machine = Data.define(:name, :entry_point, :types, :functions, :keywords) do
      def initialize(name:, entry_point: nil, types: [], functions: [], keywords: []) = super
    end

    # Type declaration: |type[Name] KIND
    TypeDecl = Data.define(:name, :kind, :lineno) do
      def initialize(name:, kind:, lineno: 0) = super
    end

    # Function definition
    Function = Data.define(:name, :return_type, :params, :states, :eof_handler, :entry_actions, :lineno) do
      def initialize(name:, return_type: nil, params: [], states: [], eof_handler: nil, entry_actions: [], lineno: 0) = super
    end

    # State within a function
    State = Data.define(:name, :cases, :eof_handler, :inline_commands, :lineno) do
      def initialize(name:, cases: [], eof_handler: nil, inline_commands: [], lineno: 0) = super
    end

    # Case within a state: |c[chars], |default, or |if[condition]
    Case = Data.define(:chars, :condition, :substate, :commands, :lineno) do
      def initialize(chars: nil, condition: nil, substate: nil, commands: [], lineno: 0) = super

      def default?     = chars.nil? && condition.nil?
      def conditional? = !condition.nil?
    end

    # EOF handler
    EOFHandler = Data.define(:commands, :lineno) do
      def initialize(commands: [], lineno: 0) = super
    end

    # Command/action within a case
    Command = Data.define(:type, :value, :lineno) do
      def initialize(type:, value: nil, lineno: 0) = super
    end

    # Conditional: |if[cond] ... |endif
    Conditional = Data.define(:clauses, :lineno) do
      def initialize(clauses: [], lineno: 0) = super
    end

    # A clause within a conditional
    Clause = Data.define(:condition, :commands) do
      def initialize(condition: nil, commands: []) = super
    end

    # Keywords block for phf perfect hash lookup
    # Example: |keywords :fallback /bare_string
    #            | true  => BoolTrue
    #            | false => BoolFalse
    Keywords = Data.define(:name, :fallback, :mappings, :lineno) do
      # name: identifier for the keyword map (e.g., "bare" generates BARE_KEYWORDS)
      # fallback: function to call when no keyword matches (e.g., "/bare_string")
      # mappings: Array of {keyword: "string", event_type: "TypeName"}
      def initialize(name:, fallback: nil, mappings: [], lineno: 0) = super
    end
  end
end
