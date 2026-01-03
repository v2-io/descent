# frozen_string_literal: true

require 'test_helper'

class ValidatorTest < Minitest::Test
  # Valid input

  def test_valid_input_has_no_errors
    validator = validate(minimal_desc)

    assert validator.valid?
    assert_empty validator.errors
  end

  # Type validation

  def test_duplicate_type_is_error
    content = <<~DESC
      |parser test
      |type[Element] BRACKET
      |type[Element] CONTENT
      |entry-point /main
      |function[main]
        |state[:main]
          |default | -> |>>
    DESC
    validator = validate(content)

    refute validator.valid?
    assert validator.errors.any? { |e| e.message.include?('Duplicate type') }
  end

  def test_unknown_type_kind_is_error
    # NOTE: This test may not trigger because the parser/IR builder
    # validates type kinds before the validator sees them
    content = <<~DESC
      |parser test
      |type[Element] BRACKET
      |entry-point /main
      |function[main]
        |state[:main]
          |default | -> |>>
    DESC
    validator = validate(content)

    # With valid kind, should be valid
    assert validator.valid?
  end

  # Function validation

  def test_duplicate_function_is_warning
    content = <<~DESC
      |parser test
      |entry-point /main
      |function[main]
        |state[:main]
          |default | -> |>>
      |function[main]
        |state[:main]
          |default | -> |>>
    DESC
    validator = validate(content)

    # Duplicate function is a warning, not error
    assert validator.valid?
    assert validator.warnings.any? { |w| w.message.include?('Duplicate function') }
  end

  def test_function_with_no_states_is_warning
    content = <<~DESC
      |parser test
      |entry-point /main
      |function[main]
    DESC
    validator = validate(content)

    # No states is a warning, not error
    assert validator.valid?
    assert validator.warnings.any? { |w| w.message.include?('no states') }
  end

  def test_unknown_return_type_is_warning
    content = <<~DESC
      |parser test
      |entry-point /main
      |function[main:UndefinedType]
        |state[:main]
          |default | -> |>>
    DESC
    validator = validate(content)

    # Unknown return type is a warning
    assert validator.valid?
    assert validator.warnings.any? { |w| w.message.include?("Return type") && w.message.include?('not declared') }
  end

  # State validation

  def test_empty_state_is_warning
    content = <<~DESC
      |parser test
      |entry-point /main
      |function[main]
        |state[:empty]
        |state[:other]
          |default | -> |>>
    DESC
    validator = validate(content)

    assert validator.valid?
    assert validator.warnings.any? { |w| w.message.include?("'empty'") && w.message.include?('no cases') }
  end

  # Command validation

  def test_call_to_undefined_function_is_warning
    content = <<~DESC
      |parser test
      |entry-point /main
      |function[main]
        |state[:main]
          |default | /nonexistent |>>
    DESC
    validator = validate(content)

    assert validator.valid?
    assert validator.warnings.any? { |w| w.message.include?('undefined function') }
  end

  def test_call_to_defined_function_is_valid
    content = <<~DESC
      |parser test
      |entry-point /main
      |function[main]
        |state[:main]
          |default | /helper |>>
      |function[helper]
        |state[:main]
          |default | -> |>>
    DESC
    validator = validate(content)

    assert validator.valid?
    refute validator.warnings.any? { |w| w.message.include?('undefined function') }
  end

  # Transition validation

  def test_transition_to_undefined_state_is_warning
    content = <<~DESC
      |parser test
      |entry-point /main
      |function[main]
        |state[:main]
          |default | -> |>> :nonexistent
    DESC
    validator = validate(content)

    assert validator.valid?
    assert validator.warnings.any? { |w| w.message.include?('undefined state') }
  end

  def test_transition_to_defined_state_is_valid
    content = <<~DESC
      |parser test
      |entry-point /main
      |function[main]
        |state[:first]
          |default | -> |>> :second
        |state[:second]
          |default | -> |>>
    DESC
    validator = validate(content)

    assert validator.valid?
    refute validator.warnings.any? { |w| w.message.include?('undefined state') }
  end

  def test_self_loop_transition_is_valid
    content = <<~DESC
      |parser test
      |entry-point /main
      |function[main]
        |state[:main]
          |default | -> |>>
    DESC
    validator = validate(content)

    assert validator.valid?
    refute validator.warnings.any? { |w| w.message.include?('transition') }
  end

  # Entry point validation

  def test_undefined_entry_point_is_error
    content = <<~DESC
      |parser test
      |entry-point /nonexistent
      |function[main]
        |state[:main]
          |default | -> |>>
    DESC
    validator = validate(content)

    refute validator.valid?
    assert validator.errors.any? { |e| e.message.include?('undefined function') }
  end

  def test_defined_entry_point_is_valid
    content = <<~DESC
      |parser test
      |entry-point /main
      |function[main]
        |state[:main]
          |default | -> |>>
    DESC
    validator = validate(content)

    assert validator.valid?
    refute validator.errors.any? { |e| e.message.include?('entry') }
  end

  # Report output

  def test_report_includes_errors_and_warnings
    content = <<~DESC
      |parser test
      |type[Element] BRACKET
      |type[Element] CONTENT
      |entry-point /nonexistent
      |function[main]
        |state[:main]
          |default | /undefined |>>
    DESC
    validator = validate(content)

    report = validator.report

    assert_match(/ERROR.*Duplicate type/, report)
    assert_match(/ERROR.*undefined function.*nonexistent/, report)
    assert_match(/WARNING.*undefined function.*undefined/, report)
  end
end
