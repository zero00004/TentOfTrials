# frozen_string_literal: true

require 'json'
require 'thread'
require 'time'

# Shared support for the v2 market stream service.
module MarketStreamSupport
  REDIS_CHANNELS = {
    trades: 'market:trades',
    orders: 'market:orders',
    ticker: 'market:ticker',
  }.freeze

  CHANNEL_BY_TYPE = {
    'trade' => REDIS_CHANNELS[:trades],
    'trades' => REDIS_CHANNELS[:trades],
    'order' => REDIS_CHANNELS[:orders],
    'orders' => REDIS_CHANNELS[:orders],
    'orderbook' => REDIS_CHANNELS[:orders],
    'order_book' => REDIS_CHANNELS[:orders],
    'tick' => REDIS_CHANNELS[:ticker],
    'ticker' => REDIS_CHANNELS[:ticker],
    'quote' => REDIS_CHANNELS[:ticker],
  }.freeze

  # Reconnection attempts are one-based at call sites. Attempt 1 therefore
  # waits 8s, satisfying the issue requirement that the first retry is at least
  # 5s while using min(2 ** (attempt + 2), 300) capped at five minutes.
  def self.reconnect_delay(attempt)
    [2**(attempt + 2), 300].min
  end

  def self.stringify_keys(value)
    case value
    when Hash
      value.each_with_object({}) do |(key, nested), memo|
        memo[key.to_s] = stringify_keys(nested)
      end
    when Array
      value.map { |nested| stringify_keys(nested) }
    else
      value
    end
  end

  def self.channel_for(message)
    return nil unless message.is_a?(Hash)

    type = message[:type] || message['type']
    CHANNEL_BY_TYPE[type.to_s.downcase]
  end

  def self.normalized_payload(message, channel, now: Time.now.utc)
    payload = stringify_keys(message)
    payload['published_at'] ||= now.iso8601(3)
    payload['redis_channel'] = channel
    payload
  end
end

# Owns Redis pub/sub connectivity for the v2 market stream service.
#
# Redis pub/sub uses a dedicated subscriber connection because SUBSCRIBE blocks;
# publishing uses a separate connection. The reconnect loop is intentionally
# independent from the exchange WebSocket reconnect loop so a Redis restart does
# not crash or stall the market stream client.
class RedisPubSubBridge
  attr_reader :last_ping_ms

  def initialize(
    redis_url: ENV.fetch('REDIS_URL', 'redis://localhost:6379/0'),
    timeout: 5,
    logger: nil,
    redis_factory: nil,
    sleep_proc: nil
  )
    @redis_url = redis_url
    @timeout = timeout
    @logger = logger
    @redis_factory = redis_factory || lambda {
      Redis.new(url: @redis_url, timeout: @timeout, reconnect_attempts: 0)
    }
    @sleep_proc = sleep_proc || ->(delay) { interruptible_sleep(delay) }

    @mutex = Mutex.new
    @thread = nil
    @stop = false
    @subscriber = nil
    @publisher = nil
    @connected = false
    @last_ping_ms = 0
    @reconnect_attempt = 0
  end

  def start
    @mutex.synchronize do
      @stop = false
      return if @thread&.alive?

      @thread = Thread.new { reconnect_loop }
      @thread.abort_on_exception = false
    end
    true
  end

  def stop
    @mutex.synchronize { @stop = true }
    close_connections
    @thread&.join(1)
    @thread&.kill if @thread&.alive?
    @thread = nil
    true
  end

  def connected?
    @mutex.synchronize { @connected }
  end

  alias connected connected?

  def ping
    redis = @mutex.synchronize { @publisher }
    return false unless redis && connected?

    start = monotonic_ms
    redis.ping
    @mutex.synchronize { @last_ping_ms = monotonic_ms - start }
    true
  rescue StandardError => e
    handle_disconnect(e)
    false
  end

  def publish_market_data(data)
    if data.is_a?(Array)
      return data.map { |message| publish_market_data(message) }.all?
    end

    channel = MarketStreamSupport.channel_for(data)
    return false unless channel

    payload = MarketStreamSupport.normalized_payload(data, channel)
    publish(channel, payload)
  end

  private

  def reconnect_loop
    until stopped?
      begin
        connect!
        subscribe!
      rescue StandardError => e
        handle_disconnect(e) unless stopped?
      ensure
        close_connections unless stopped? && connected?
      end

      break if stopped?

      @reconnect_attempt += 1
      delay = MarketStreamSupport.reconnect_delay(@reconnect_attempt)
      log(:warn, "Redis pub/sub disconnected; reconnecting in #{delay}s (attempt #{@reconnect_attempt})")
      @sleep_proc.call(delay)
    end
  end

  def connect!
    subscriber = @redis_factory.call
    publisher = @redis_factory.call

    start = monotonic_ms
    publisher.ping
    ping_ms = monotonic_ms - start

    @mutex.synchronize do
      @subscriber = subscriber
      @publisher = publisher
      @last_ping_ms = ping_ms
      @connected = true
      @reconnect_attempt = 0
    end

    log(:info, "Connected to Redis pub/sub at #{@redis_url} (ping #{ping_ms}ms)")
  rescue StandardError
    close_redis(subscriber)
    close_redis(publisher)
    raise
  end

  def subscribe!
    subscriber = @mutex.synchronize { @subscriber }
    subscriber.subscribe(*MarketStreamSupport::REDIS_CHANNELS.values) do |on|
      on.subscribe do |channel, subscriptions|
        log(:info, "Subscribed to Redis channel #{channel} (#{subscriptions} total)")
      end

      on.message do |channel, message|
        log(:debug, "Redis pub/sub message on #{channel}: #{message}")
      end
    end
  end

  def publish(channel, payload)
    redis = @mutex.synchronize { @publisher }
    return false unless redis && connected?

    redis.publish(channel, JSON.generate(payload))
    true
  rescue StandardError => e
    handle_disconnect(e)
    false
  end

  def handle_disconnect(error)
    was_connected = @mutex.synchronize do
      was_connected = @connected
      @connected = false
      was_connected
    end

    log(was_connected ? :warn : :debug, "Redis pub/sub connection lost: #{error.class}: #{error.message}")
    close_connections
  end

  def close_connections
    subscriber = nil
    publisher = nil
    @mutex.synchronize do
      subscriber = @subscriber
      publisher = @publisher
      @subscriber = nil
      @publisher = nil
      @connected = false
    end

    close_redis(subscriber)
    close_redis(publisher)
  end

  def close_redis(redis)
    return unless redis

    if redis.respond_to?(:close)
      redis.close
    elsif redis.respond_to?(:disconnect!)
      redis.disconnect!
    end
  rescue StandardError
    nil
  end

  def stopped?
    @mutex.synchronize { @stop }
  end

  def interruptible_sleep(delay)
    deadline = Time.now + delay
    until stopped? || Time.now >= deadline
      sleep([deadline - Time.now, 0.25].min)
    end
  end

  def monotonic_ms
    (Process.clock_gettime(Process::CLOCK_MONOTONIC) * 1000).round
  end

  def log(level, message)
    if @logger&.respond_to?(level)
      @logger.public_send(level, message)
    end
  end
end
