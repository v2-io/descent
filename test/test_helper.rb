# frozen_string_literal: true

$LOAD_PATH.unshift File.expand_path('../lib', __dir__)
require 'descent'
require 'minitest/autorun'

# Optional: nicer output
begin
  require 'minitest/reporters'
  Minitest::Reporters.use! Minitest::Reporters::SpecReporter.new
rescue LoadError
  # minitest-reporters not available, use default
end

# Shared test utilities for descent tests
module DescentTestHelpers
  # Tokenize a .desc string
  def tokenize(content, source_file: '(test)')
    Descent::Lexer.new(content, source_file:).tokenize
  end

  # Parse a .desc string to AST
  def parse(content, source_file: '(test)')
    tokens = tokenize(content, source_file:)
    Descent::Parser.new(tokens).parse
  end

  # Build IR from a .desc string
  def build_ir(content, source_file: '(test)')
    ast = parse(content, source_file:)
    Descent::IRBuilder.new(ast).build
  end

  # Generate Rust code from a .desc string
  def generate(content, source_file: '(test)', **options)
    Descent.generate(content, target: :rust, **options)
  end

  # Validate IR and return validator
  def validate(content, source_file: '(test)')
    ir = build_ir(content, source_file:)
    Descent::Validator.new(ir).validate
  end

  # Path to fixtures directory
  def fixture_path(name)
    File.join(__dir__, 'fixtures', name)
  end

  # Read a fixture file
  def fixture(name)
    File.read(fixture_path(name))
  end

  # Minimal valid .desc for testing
  def minimal_desc
    <<~DESC
      |parser test
      |type[Text] CONTENT
      |entry-point /main
      |function[main:Text]
        |state[:main]
          |default | -> |>>
    DESC
  end
end

# Include helpers in all test classes
class Minitest::Test
  include DescentTestHelpers
end
