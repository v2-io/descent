# frozen_string_literal: true

require 'test_helper'

class LexerTest < Minitest::Test
  # Token structure: type, tag, id, rest, lineno
  # - type: usually :part (generic token) or :>> (transition)
  # - tag: the directive name (parser, type, function, c, ->, etc.)
  # - id: content inside brackets [...]
  # - rest: everything after the brackets

  # Basic tokenization

  def test_tokenizes_parser_directive
    tokens = tokenize('|parser myparser')
    assert_equal 1, tokens.size
    assert_equal 'parser', tokens[0].tag
    assert_equal 'myparser', tokens[0].rest
  end

  def test_tokenizes_type_declaration
    tokens = tokenize('|type[Element] BRACKET')
    assert_equal 1, tokens.size
    assert_equal 'type', tokens[0].tag
    assert_equal 'Element', tokens[0].id
    assert_equal 'BRACKET', tokens[0].rest
  end

  def test_tokenizes_entry_point
    tokens = tokenize('|entry-point /document')
    assert_equal 1, tokens.size
    assert_equal 'entry-point', tokens[0].tag
    assert_equal '/document', tokens[0].rest
  end

  def test_tokenizes_function_with_return_type
    tokens = tokenize('|function[name:ReturnType]')
    assert_equal 1, tokens.size
    assert_equal 'function', tokens[0].tag
    assert_equal 'name:ReturnType', tokens[0].id
  end

  def test_tokenizes_function_with_params
    tokens = tokenize('|function[name:Type] :p1 :p2')
    assert_equal 1, tokens.size
    assert_equal 'function', tokens[0].tag
    assert_equal 'name:Type', tokens[0].id
    assert_equal ':p1 :p2', tokens[0].rest
  end

  def test_tokenizes_state
    tokens = tokenize('|state[:main]')
    assert_equal 1, tokens.size
    assert_equal 'state', tokens[0].tag
    assert_equal ':main', tokens[0].id
  end

  # Multiple tokens per line

  def test_tokenizes_case_with_actions
    tokens = tokenize("|c['\\n'] | -> | MARK |>> :next")
    assert_equal 4, tokens.size
    assert_equal 'c', tokens[0].tag
    assert_equal "'\\n'", tokens[0].id
    assert_equal '->', tokens[1].tag
    assert_equal 'mark', tokens[2].tag
    assert_equal '>>', tokens[3].tag
  end

  def test_tokenizes_substate_label
    tokens = tokenize('|c[x] |.label | -> |>>')
    assert_equal 4, tokens.size
    assert_equal '.', tokens[1].tag
    assert_equal 'label', tokens[1].rest
  end

  # Comment handling

  def test_strips_comments
    tokens = tokenize("|parser test ; this is a comment\n|type[X] BRACKET")
    assert_equal 2, tokens.size
    assert_equal 'parser', tokens[0].tag
    assert_equal 'type', tokens[1].tag
  end

  def test_skips_comment_only_lines
    tokens = tokenize("; just a comment\n|parser test")
    assert_equal 1, tokens.size
    assert_equal 'parser', tokens[0].tag
  end

  # Bracket handling

  def test_handles_pipe_in_brackets
    tokens = tokenize("|c[<P>] | -> |>>")
    assert_equal 3, tokens.size
    assert_equal 'c', tokens[0].tag
    assert_equal '<P>', tokens[0].id
  end

  def test_handles_bracket_in_quoted_string
    tokens = tokenize("|c['['] | -> |>>")
    assert_equal 3, tokens.size
    assert_equal 'c', tokens[0].tag
    assert_equal "'['", tokens[0].id
  end

  # Quote handling

  def test_handles_escaped_quote
    tokens = tokenize("|c['\\''] | -> |>>")
    assert_equal 3, tokens.size
    assert_equal 'c', tokens[0].tag
  end

  def test_handles_pipe_in_quotes
    tokens = tokenize("|/func('|')")
    assert_equal 1, tokens.size
    assert_match(/func/, tokens[0].tag)
  end

  # Line numbers

  def test_tracks_line_numbers
    content = <<~DESC
      |parser test
      |type[X] BRACKET
      |entry-point /main
    DESC
    tokens = tokenize(content)
    assert_equal 1, tokens[0].lineno
    assert_equal 2, tokens[1].lineno
    assert_equal 3, tokens[2].lineno
  end

  # Edge cases

  def test_empty_input
    tokens = tokenize('')
    assert_empty tokens
  end

  def test_whitespace_only
    tokens = tokenize("   \n  \n  ")
    assert_empty tokens
  end

  def test_multiline_function
    content = <<~DESC
      |function[test]
        |state[:main]
          |default | -> |>>
    DESC
    tokens = tokenize(content)
    # function, state, default, ->, >>
    assert_equal 5, tokens.size
    assert_equal 'function', tokens[0].tag
    assert_equal 'state', tokens[1].tag
    assert_equal 'default', tokens[2].tag
  end

  # Error cases

  def test_raises_on_unterminated_quote
    assert_raises(Descent::LexerError) do
      tokenize("|c['unterminated")
    end
  end
end
