# frozen_string_literal: true

require 'test_helper'

class IRBuilderTest < Minitest::Test
  # Type resolution

  def test_bracket_type_emits_start_and_end
    content = <<~DESC
      |parser test
      |type[Element] BRACKET
      |entry-point /main
      |function[main:Element]
        |state[:main]
          |default | -> |>>
    DESC
    ir = build_ir(content)
    type = ir.types.find { |t| t.name == 'Element' }

    assert_equal :bracket, type.kind
    assert type.emits_start
    assert type.emits_end
  end

  def test_content_type_does_not_emit_start_or_end
    content = <<~DESC
      |parser test
      |type[Text] CONTENT
      |entry-point /main
      |function[main:Text]
        |state[:main]
          |default | -> |>>
    DESC
    ir = build_ir(content)
    type = ir.types.find { |t| t.name == 'Text' }

    assert_equal :content, type.kind
    refute type.emits_start
    refute type.emits_end
  end

  def test_internal_type_has_internal_kind
    content = <<~DESC
      |parser test
      |type[Counter] INTERNAL
      |entry-point /main
      |function[main:Counter]
        |state[:main]
          |default | -> |>>
    DESC
    ir = build_ir(content)
    type = ir.types.find { |t| t.name == 'Counter' }

    assert_equal :internal, type.kind
    refute type.emits_start
    refute type.emits_end
  end

  # SCAN inference

  def test_self_looping_default_is_scannable
    content = <<~DESC
      |parser test
      |entry-point /main
      |function[main]
        |state[:main]
          |c[x]    | ->  |>> :done
          |default | ->  |>>
        |state[:done]
          |default |return
    DESC
    ir = build_ir(content)
    main_state = ir.functions[0].states.find { |s| s.name == 'main' }

    assert main_state.scannable?
    assert_includes main_state.scan_chars, 'x'
  end

  def test_non_self_looping_default_is_not_scannable
    content = <<~DESC
      |parser test
      |entry-point /main
      |function[main]
        |state[:main]
          |c[x]    | -> |>> :other
          |default | -> |>> :other
        |state[:other]
          |default |return
    DESC
    ir = build_ir(content)
    main_state = ir.functions[0].states.find { |s| s.name == 'main' }

    refute main_state.scannable?
  end

  def test_scan_collects_exit_characters
    content = <<~DESC
      |parser test
      |entry-point /main
      |function[main]
        |state[:main]
          |c['\\n']  | -> |return
          |c['|']  | -> |>> :pipe
          |default | ->  |>>
        |state[:pipe]
          |default |return
    DESC
    ir = build_ir(content)
    main_state = ir.functions[0].states.find { |s| s.name == 'main' }

    assert main_state.scannable?
    assert_includes main_state.scan_chars, "\n"
    assert_includes main_state.scan_chars, '|'
  end

  # Newline injection for line tracking

  def test_newline_injected_into_scan_chars
    content = <<~DESC
      |parser test
      |entry-point /main
      |function[main]
        |state[:main]
          |c['|']  | -> |>> :pipe
          |default | ->  |>>
        |state[:pipe]
          |default |return
    DESC
    ir = build_ir(content)
    main_state = ir.functions[0].states.find { |s| s.name == 'main' }

    # Newline should be auto-injected for line tracking
    assert main_state.newline_injected
    assert_includes main_state.scan_chars, "\n"
  end

  def test_newline_not_injected_when_already_present
    content = <<~DESC
      |parser test
      |entry-point /main
      |function[main]
        |state[:main]
          |c['\\n']  | -> |>> :eol
          |default | ->  |>>
        |state[:eol]
          |default |return
    DESC
    ir = build_ir(content)
    main_state = ir.functions[0].states.find { |s| s.name == 'main' }

    # Already has newline, no injection needed
    refute main_state.newline_injected
    assert_includes main_state.scan_chars, "\n"
  end

  # Parameter type inference

  def test_param_in_char_match_becomes_byte_type
    content = <<~DESC
      |parser test
      |entry-point /main
      |function[main] :close
        |state[:main]
          |c[:close] | -> |return
          |default   | -> |>>
    DESC
    ir = build_ir(content)
    func = ir.functions[0]

    assert_equal :byte, func.param_types['close']
  end

  def test_param_in_prepend_becomes_bytes_type
    content = <<~DESC
      |parser test
      |entry-point /main
      |function[main] :prefix
        |state[:main]
          |c[x]    | PREPEND(:prefix) |return
          |default | -> |>>
    DESC
    ir = build_ir(content)
    func = ir.functions[0]

    assert_equal :bytes, func.param_types['prefix']
  end

  def test_param_in_condition_becomes_i32_type
    content = <<~DESC
      |parser test
      |entry-point /main
      |function[main] :col
        |state[:main]
          |if[COL <= :col] |return
          |default | -> |>>
    DESC
    ir = build_ir(content)
    func = ir.functions[0]

    assert_equal :i32, func.param_types['col']
  end

  # Local variables (via function entry actions)

  def test_detects_local_variable_from_entry_action
    content = <<~DESC
      |parser test
      |entry-point /main
      |function[main] | depth = 1
        |state[:main]
          |default | -> |>>
    DESC
    ir = build_ir(content)
    func = ir.functions[0]

    # Locals is a Hash: { var_name => type }
    assert func.locals.key?('depth')
    assert_equal :i32, func.locals['depth']
  end

  # expects_char inference

  def test_infers_expects_char_from_single_return_case
    content = <<~DESC
      |parser test
      |type[StringValue] CONTENT
      |entry-point /main
      |function[main:StringValue]
        |state[:main]
          |c['"']   | -> |return
          |default | -> |>>
    DESC
    ir = build_ir(content)
    func = ir.functions[0]

    assert_equal '"', func.expects_char
  end

  def test_no_expects_char_when_multiple_return_chars
    content = <<~DESC
      |parser test
      |type[Value] CONTENT
      |entry-point /main
      |function[main:Value]
        |state[:main]
          |c['"']  | -> |return
          |c[')']  | -> |return
          |default | -> |>>
    DESC
    ir = build_ir(content)
    func = ir.functions[0]

    # Multiple different return chars - can't infer single expects_char
    assert_nil func.expects_char
  end

  # Entry actions

  def test_preserves_function_entry_actions
    content = <<~DESC
      |parser test
      |entry-point /main
      |function[main] | depth = 1
        |state[:main]
          |default | -> |>>
    DESC
    ir = build_ir(content)
    func = ir.functions[0]

    # Entry actions are Command objects with .type
    assert func.entry_actions.any? { |cmd| cmd.type == :assign }
  end

  # EOF handler

  def test_eof_handler_is_preserved
    content = <<~DESC
      |parser test
      |type[Number] CONTENT
      |entry-point /main
      |function[main:Number]
        |state[:main]
          |DIGIT   | -> |>>
          |default | TERM |return
          |eof     | TERM |return
    DESC
    ir = build_ir(content)
    state = ir.functions[0].states[0]

    # EOF handler is an array of Command objects directly
    refute_nil state.eof_handler
    refute_empty state.eof_handler
    assert state.eof_handler.any? { |cmd| cmd.type == :term }
  end

  # Unconditional states

  def test_bare_action_case_is_unconditional
    content = <<~DESC
      |parser test
      |entry-point /main
      |function[main]
        |state[:setup] | MARK
          | MARK |>> :process
        |state[:process]
          |default | -> |>>
    DESC
    ir = build_ir(content)
    setup_state = ir.functions[0].states.find { |s| s.name == 'setup' }

    # State has bare action case (no char match) - should be unconditional
    assert setup_state.is_unconditional
  end

  # Keywords

  def test_keywords_block_creates_ir_keywords
    content = <<~DESC
      |parser test
      |entry-point /main
      |keywords[bare] :fallback /identifier
        | true  => BoolTrue
        | false => BoolFalse
      |function[main]
        |state[:main]
          |default | -> |>>
      |function[identifier]
        |state[:main]
          |default | -> |>>
    DESC
    ir = build_ir(content)

    assert_equal 1, ir.keywords.size
    kw = ir.keywords[0]
    assert_equal 'bare', kw.name
    assert_equal 'identifier', kw.fallback_func
    assert_equal 2, kw.mappings.size
    assert_equal 'true', kw.mappings[0][:keyword]
    assert_equal 'BoolTrue', kw.mappings[0][:event_type]
  end
end
