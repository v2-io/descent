# frozen_string_literal: true

require 'test_helper'
require 'descent/generator'

class GeneratorTest < Minitest::Test
  # Liquid filter tests

  def test_escape_rust_char_newline
    assert_equal "b'\\n'", filter.escape_rust_char("\n")
  end

  def test_escape_rust_char_tab
    assert_equal "b'\\t'", filter.escape_rust_char("\t")
  end

  def test_escape_rust_char_single_quote
    assert_equal "b'\\''", filter.escape_rust_char("'")
  end

  def test_escape_rust_char_regular_char
    assert_equal "b'x'", filter.escape_rust_char('x')
  end

  def test_escape_rust_char_nil
    assert_equal "b'?'", filter.escape_rust_char(nil)
  end

  def test_pascalcase_snake_case
    assert_equal 'AfterName', filter.pascalcase('after_name')
  end

  def test_pascalcase_single_word
    assert_equal 'Main', filter.pascalcase('main')
  end

  def test_pascalcase_preserves_existing
    assert_equal 'UnclosedInterpolation', filter.pascalcase('UnclosedInterpolation')
  end

  def test_pascalcase_nil
    assert_equal '', filter.pascalcase(nil)
  end

  def test_rust_expr_col
    assert_equal 'self.col()', filter.rust_expr('COL')
  end

  def test_rust_expr_line
    assert_equal 'self.line as i32', filter.rust_expr('LINE')
  end

  def test_rust_expr_prev
    assert_equal 'self.prev()', filter.rust_expr('PREV')
  end

  def test_rust_expr_param_reference
    assert_equal 'col', filter.rust_expr(':col')
  end

  def test_rust_expr_function_call
    assert_equal 'self.parse_helper(on_event)', filter.rust_expr('/helper')
  end

  def test_rust_expr_function_call_with_args
    # COL in function args should work correctly
    result = filter.rust_expr('/element(COL)')
    assert_equal 'self.parse_element(self.col(), on_event)', result
  end

  def test_rust_expr_function_call_with_param
    result = filter.rust_expr('/element(:col)')
    assert_equal 'self.parse_element(col, on_event)', result
  end

  def test_rust_expr_escape_sequences
    assert_equal "b'|'", filter.rust_expr('<P>')
    assert_equal "b']'", filter.rust_expr('<R>')
    assert_equal "b'['", filter.rust_expr('<L>')
    assert_equal "b'}'", filter.rust_expr('<RB>')
    assert_equal "b'{'", filter.rust_expr('<LB>')
  end

  def test_rust_expr_condition
    result = filter.rust_expr('COL <= :col')
    assert_equal 'self.col() <= col', result
  end

  # Generated code structure tests

  def test_generates_event_enum
    rust_code = generate(minimal_desc)

    assert_match(/pub enum Event/, rust_code)
  end

  def test_generates_parser_struct
    rust_code = generate(minimal_desc)

    assert_match(/pub struct Parser/, rust_code)
  end

  def test_generates_parse_method
    rust_code = generate(minimal_desc)

    assert_match(/pub fn parse/, rust_code)
  end

  def test_generates_entry_function
    rust_code = generate(minimal_desc)

    assert_match(/parse_main/, rust_code)
  end

  # Type-driven event generation

  def test_bracket_type_generates_start_and_end
    content = <<~DESC
      |parser test
      |type[Element] BRACKET
      |entry-point /main
      |function[main:Element]
        |state[:main]
          |default | -> |>>
    DESC
    rust_code = generate(content)

    assert_match(/ElementStart/, rust_code)
    assert_match(/ElementEnd/, rust_code)
  end

  def test_content_type_generates_content_event
    rust_code = generate(minimal_desc)

    # minimal_desc has Text as CONTENT type
    assert_match(/Text\s*\{/, rust_code)
  end

  # Error code generation

  def test_generates_error_code_enum
    rust_code = generate(minimal_desc)

    assert_match(/pub enum ParseErrorCode/, rust_code)
  end

  def test_generates_unclosed_error_for_expects_char
    content = <<~DESC
      |parser test
      |type[StringValue] CONTENT
      |entry-point /main
      |function[main:StringValue]
        |state[:main]
          |c['"'] | -> |return
          |default | -> |>>
    DESC
    rust_code = generate(content)

    assert_match(/UnclosedStringValue/, rust_code)
  end

  # Helper method conditional emission

  def test_generates_prev_helper_when_used
    content = <<~DESC
      |parser test
      |entry-point /main
      |function[main]
        |state[:main]
          |if[PREV == ' '] |return
          |default | -> |>>
    DESC
    rust_code = generate(content)

    assert_match(/fn prev\(&self\) -> u8/, rust_code)
  end

  def test_generates_col_helper_when_used
    content = <<~DESC
      |parser test
      |entry-point /main
      |function[main] :col
        |state[:main]
          |if[COL <= :col] |return
          |default | -> |>>
    DESC
    rust_code = generate(content)

    assert_match(/fn col\(&self\) -> i32/, rust_code)
  end

  # SCAN optimization

  def test_generates_scan_for_scannable_state
    content = <<~DESC
      |parser test
      |entry-point /main
      |function[main]
        |state[:main]
          |c['\\n'] | -> |return
          |default | -> |>>
    DESC
    rust_code = generate(content)

    # Should use memchr for scanning
    assert_match(/memchr/, rust_code)
  end

  # Trace mode

  def test_trace_mode_includes_trace_output
    content = minimal_desc
    rust_code = Descent.generate(content, target: :rust, trace: true)

    assert_match(/TRACE:/, rust_code)
    assert_match(/eprintln!/, rust_code)  # eprintln with 'ln'
  end

  def test_normal_mode_no_trace_output
    rust_code = generate(minimal_desc)

    refute_match(/TRACE:/, rust_code)
  end

  # Keywords (phf)

  def test_generates_phf_map_for_keywords
    content = <<~DESC
      |parser test
      |entry-point /main
      |keywords[bare] :fallback /identifier
        | true  => BoolTrue
        | false => BoolFalse
      |function[main]
        |state[:main]
          |LABEL_CONT | -> |>>
          |default | TERM | KEYWORDS(bare) |return
      |function[identifier]
        |state[:main]
          |LABEL_CONT | -> |>>
          |default |return
    DESC
    rust_code = generate(content)

    assert_match(/phf_map!/, rust_code)
    assert_match(/b"true"/, rust_code)
    assert_match(/b"false"/, rust_code)
  end

  private

  # Create a filter instance for testing
  def filter
    @filter ||= Object.new.extend(Descent::LiquidFilters)
  end
end
