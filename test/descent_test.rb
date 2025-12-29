# frozen_string_literal: true

require "test_helper"

class DescentTest < Minitest::Test
  def test_version_exists
    refute_nil Descent::VERSION
  end
end
