# frozen_string_literal: true

require 'minitest/autorun'
require 'json'
require_relative '../v2/services/market_stream_support'

class FakeRedis
  attr_reader :published, :subscribed_channels

  def initialize(subscribe_error: nil)
    @subscribe_error = subscribe_error
    @closed = false
    @published = []
    @subscribed_channels = []
  end

  def ping
    raise IOError, 'closed' if @closed

    'PONG'
  end

  def publish(channel, payload)
    raise IOError, 'closed' if @closed

    @published << [channel, JSON.parse(payload)]
    1
  end

  def subscribe(*channels)
    raise @subscribe_error if @subscribe_error

    @subscribed_channels = channels
    callbacks = FakeSubscriptionCallbacks.new
    yield callbacks
    sleep 0.01 until @closed
  end

  def close
    @closed = true
  end
end

class FakeSubscriptionCallbacks
  def subscribe
    yield('market:trades', 1) if block_given?
  end

  def message; end
end

class RedisPubSubBridgeTest < Minitest::Test
  def teardown
    @bridge&.stop
  end

  def test_backoff_uses_one_based_exponential_formula_capped_at_five_minutes
    assert_equal 8, MarketStreamSupport.reconnect_delay(1)
    assert_equal 16, MarketStreamSupport.reconnect_delay(2)
    assert_equal 300, MarketStreamSupport.reconnect_delay(10)
  end

  def test_subscribes_to_required_market_channels_on_startup
    redis = []
    @bridge = RedisPubSubBridge.new(
      redis_factory: -> { FakeRedis.new.tap { |client| redis << client } },
      sleep_proc: ->(_delay) {}
    )

    @bridge.start
    wait_until { redis.first&.subscribed_channels&.any? }

    assert_equal %w[market:trades market:orders market:ticker], redis.first.subscribed_channels
    assert @bridge.connected?
  end

  def test_publishes_normalized_market_data_to_corresponding_channels
    redis = []
    @bridge = RedisPubSubBridge.new(
      redis_factory: -> { FakeRedis.new.tap { |client| redis << client } },
      sleep_proc: ->(_delay) {}
    )
    @bridge.start
    wait_until { @bridge.connected? }

    assert @bridge.publish_market_data(type: 'trade', instrument: 'BTC/USD', price: 42)
    assert @bridge.publish_market_data('type' => 'tick', 'instrument' => 'ETH/USD', 'bid' => 10)
    publisher = redis[1]

    assert_equal 'market:trades', publisher.published[0][0]
    assert_equal 'trade', publisher.published[0][1]['type']
    assert_equal 'market:trades', publisher.published[0][1]['redis_channel']
    assert publisher.published[0][1]['published_at']

    assert_equal 'market:ticker', publisher.published[1][0]
    assert_equal 'market:ticker', publisher.published[1][1]['redis_channel']
  end

  def test_reconnects_after_subscriber_disconnect_with_backoff_independent_of_publish_connection
    queue = [
      FakeRedis.new(subscribe_error: IOError.new('redis restart')),
      FakeRedis.new,
      FakeRedis.new,
      FakeRedis.new,
    ]
    created = []
    delays = []
    @bridge = RedisPubSubBridge.new(
      redis_factory: -> { queue.shift.tap { |client| created << client } },
      sleep_proc: ->(delay) { delays << delay }
    )

    @bridge.start
    wait_until { @bridge.connected? && delays == [8] && created[2]&.subscribed_channels&.any? }

    assert_equal [8], delays
    assert_equal %w[market:trades market:orders market:ticker], created[2].subscribed_channels
    assert @bridge.publish_market_data(type: 'order', instrument: 'BTC/USD', side: 'buy')
    assert_equal 'market:orders', created[3].published.first[0]
  end

  private

  def wait_until(timeout: 1.0)
    deadline = Time.now + timeout
    until yield
      raise 'timed out waiting for condition' if Time.now >= deadline

      sleep 0.01
    end
  end
end
