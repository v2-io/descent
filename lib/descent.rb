# frozen_string_literal: true

require_relative "descent/version"

module Descent
  class Error < StandardError; end
  class ParseError < Error; end
  class ValidationError < Error; end

  autoload :AST,       "descent/ast"
  autoload :IR,        "descent/ir"
  autoload :Lexer,     "descent/lexer"
  autoload :Parser,    "descent/parser"
  autoload :IRBuilder, "descent/ir_builder"
  autoload :Generator, "descent/generator"

  # Main entry point: parse a .desc file and generate output
  #
  # @param input [String] Path to .desc file or string content
  # @param target [Symbol] Target language (:rust, :c)
  # @param options [Hash] Additional options
  # @return [String] Generated parser code
  def self.generate(input, target:, **options)
    content     = File.exist?(input) ? File.read(input) : input
    source_file = File.exist?(input) ? input : "(string)"

    tokens = Lexer.new(content, source_file:).tokenize
    ast    = Parser.new(tokens).parse
    ir     = IRBuilder.new(ast).build

    Generator.new(ir, target:, **options).generate
  end
end
