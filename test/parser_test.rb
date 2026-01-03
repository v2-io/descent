# frozen_string_literal: true

require 'test_helper'

class ParserTest < Minitest::Test
  # Parser directive

  def test_parses_parser_name
    ast = parse('|parser myparser')
    assert_equal 'myparser', ast.name
  end

  # Type declarations

  def test_parses_type_bracket
    ast = parse("|parser test\n|type[Element] BRACKET")
    assert_equal 1, ast.types.size
    assert_equal 'Element', ast.types[0].name
    assert_equal :BRACKET, ast.types[0].kind
  end

  def test_parses_type_content
    ast = parse("|parser test\n|type[Text] CONTENT")
    assert_equal 1, ast.types.size
    assert_equal 'Text', ast.types[0].name
    assert_equal :CONTENT, ast.types[0].kind
  end

  def test_parses_type_internal
    ast = parse("|parser test\n|type[Counter] INTERNAL")
    assert_equal 1, ast.types.size
    assert_equal 'Counter', ast.types[0].name
    assert_equal :INTERNAL, ast.types[0].kind
  end

  def test_parses_multiple_types
    content = <<~DESC
      |parser test
      |type[Element] BRACKET
      |type[Text] CONTENT
      |type[Count] INTERNAL
    DESC
    ast = parse(content)
    assert_equal 3, ast.types.size
  end

  # Entry point

  def test_parses_entry_point
    ast = parse("|parser test\n|entry-point /document")
    assert_equal '/document', ast.entry_point
  end

  # Functions

  def test_parses_void_function
    content = <<~DESC
      |parser test
      |entry-point /main
      |function[main]
        |state[:start]
          |default | -> |>>
    DESC
    ast = parse(content)
    assert_equal 1, ast.functions.size
    assert_equal 'main', ast.functions[0].name
    assert_nil ast.functions[0].return_type
  end

  def test_parses_function_with_return_type
    content = <<~DESC
      |parser test
      |type[Text] CONTENT
      |entry-point /main
      |function[main:Text]
        |state[:start]
          |default | -> |>>
    DESC
    ast = parse(content)
    assert_equal 'main', ast.functions[0].name
    assert_equal 'Text', ast.functions[0].return_type
  end

  def test_parses_function_with_params
    content = <<~DESC
      |parser test
      |entry-point /main
      |function[main] :p1 :p2
        |state[:start]
          |default | -> |>>
    DESC
    ast = parse(content)
    func = ast.functions[0]
    assert_equal 2, func.params.size
    # Params are stored as strings (the IR builder enriches them later)
    assert_equal 'p1', func.params[0]
    assert_equal 'p2', func.params[1]
  end

  # States

  def test_parses_state
    content = <<~DESC
      |parser test
      |entry-point /main
      |function[main]
        |state[:first]
          |default | -> |>>
        |state[:second]
          |default | -> |>>
    DESC
    ast = parse(content)
    func = ast.functions[0]
    assert_equal 2, func.states.size
    assert_equal 'first', func.states[0].name
    assert_equal 'second', func.states[1].name
  end

  # Cases

  def test_parses_character_case
    content = <<~DESC
      |parser test
      |entry-point /main
      |function[main]
        |state[:start]
          |c[x] | -> |>>
    DESC
    ast = parse(content)
    kase = ast.functions[0].states[0].cases[0]
    assert_equal 'x', kase.chars
    refute kase.default?
  end

  def test_parses_default_case
    content = <<~DESC
      |parser test
      |entry-point /main
      |function[main]
        |state[:start]
          |default | -> |>>
    DESC
    ast = parse(content)
    kase = ast.functions[0].states[0].cases[0]
    assert kase.default?
  end

  def test_parses_substate_label
    content = <<~DESC
      |parser test
      |entry-point /main
      |function[main]
        |state[:start]
          |c[x] |.label | -> |>>
    DESC
    ast = parse(content)
    kase = ast.functions[0].states[0].cases[0]
    assert_equal 'label', kase.substate
  end

  def test_parses_special_class_case
    content = <<~DESC
      |parser test
      |entry-point /main
      |function[main]
        |state[:start]
          |LETTER | -> |>>
    DESC
    ast = parse(content)
    kase = ast.functions[0].states[0].cases[0]
    assert_equal 'LETTER', kase.chars
  end

  # Commands

  def test_parses_advance_command
    content = <<~DESC
      |parser test
      |entry-point /main
      |function[main]
        |state[:start]
          |default | -> |>>
    DESC
    ast = parse(content)
    cmds = ast.functions[0].states[0].cases[0].commands
    assert_equal :advance, cmds[0].type
  end

  def test_parses_advance_to_command
    content = <<~DESC
      |parser test
      |entry-point /main
      |function[main]
        |state[:start]
          |default | ->['\\n'] |>>
    DESC
    ast = parse(content)
    cmds = ast.functions[0].states[0].cases[0].commands
    assert_equal :advance_to, cmds[0].type
    assert_equal "'\\n'", cmds[0].value
  end

  def test_parses_function_call
    content = <<~DESC
      |parser test
      |entry-point /main
      |function[main]
        |state[:start]
          |default | /other |>>
      |function[other]
        |state[:start]
          |default | -> |>>
    DESC
    ast = parse(content)
    cmds = ast.functions[0].states[0].cases[0].commands
    assert_equal :call, cmds[0].type
    # Value is the function name without leading slash
    assert_equal 'other', cmds[0].value
  end

  def test_parses_mark_term_commands
    content = <<~DESC
      |parser test
      |entry-point /main
      |function[main]
        |state[:start]
          |default | MARK | -> | TERM |>>
    DESC
    ast = parse(content)
    cmds = ast.functions[0].states[0].cases[0].commands
    assert_equal :mark, cmds[0].type
    assert_equal :advance, cmds[1].type
    assert_equal :term, cmds[2].type
  end

  # Transitions

  def test_parses_self_loop_transition
    content = <<~DESC
      |parser test
      |entry-point /main
      |function[main]
        |state[:start]
          |default | -> |>>
    DESC
    ast = parse(content)
    cmds = ast.functions[0].states[0].cases[0].commands
    transition = cmds.find { |c| c.type == :transition }
    assert_equal '', transition.value
  end

  def test_parses_named_transition
    content = <<~DESC
      |parser test
      |entry-point /main
      |function[main]
        |state[:start]
          |c[x] | -> |>> :other
        |state[:other]
          |default | -> |>>
    DESC
    ast = parse(content)
    cmds = ast.functions[0].states[0].cases[0].commands
    transition = cmds.find { |c| c.type == :transition }
    assert_equal ':other', transition.value
  end

  def test_parses_return
    content = <<~DESC
      |parser test
      |entry-point /main
      |function[main]
        |state[:start]
          |default | -> |return
    DESC
    ast = parse(content)
    cmds = ast.functions[0].states[0].cases[0].commands
    ret = cmds.find { |c| c.type == :return }
    refute_nil ret
  end

  # EOF handling

  def test_parses_eof_case
    content = <<~DESC
      |parser test
      |entry-point /main
      |function[main]
        |state[:start]
          |default | -> |>>
          |eof | TERM |return
    DESC
    ast = parse(content)
    state = ast.functions[0].states[0]
    refute_nil state.eof_handler
    assert_equal :term, state.eof_handler.commands[0].type
  end

  # Keywords

  def test_parses_keywords_block
    content = <<~DESC
      |parser test
      |entry-point /main
      |keywords[bare] :fallback /identifier
        | true  => BoolTrue
        | false => BoolFalse
      |function[main]
        |state[:start]
          |default | -> |>>
    DESC
    ast = parse(content)
    assert_equal 1, ast.keywords.size
    kw = ast.keywords[0]
    assert_equal 'bare', kw.name
    assert_equal '/identifier', kw.fallback
    assert_equal 2, kw.mappings.size
    assert_equal 'true', kw.mappings[0][:keyword]
    assert_equal 'BoolTrue', kw.mappings[0][:event_type]
  end

  # Error handling

  # Parser allows missing parser name (returns nil) - validator catches this
  def test_handles_missing_parser_name
    ast = parse('|type[X] BRACKET')
    assert_nil ast.name
  end
end
