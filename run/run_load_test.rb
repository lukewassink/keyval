# frozen_string_literal: true

require 'pathname'

puts "\nSetting up load test..."
puts ''

# File names
db_config = 'run/config/db_config.toml'
lt_config = 'run/config/lt_config.toml'
db_preffix = 'testdb_'
lt_preffix = 'loadtest_'

# Compute file paths
db_pref_len = db_preffix.length
lt_pref_len = lt_preffix.length

root = Pathname.new(__dir__) / '..'
puts root
testdbs = (root / 'run/dbs').children.map { |path| path.basename.to_s }.select { |file| file.start_with?(db_preffix) }
loadtest_outputs = (root / 'run/tests').children.map do |path|
  path.basename.to_s
end.select { |file| file.start_with?(lt_preffix) }
puts testdbs
puts loadtest_outputs
max_db = testdbs.map { |file| file[db_pref_len..].to_i }.max | 0
max_lt = loadtest_outputs.map { |file| file[lt_pref_len..-5].to_i }.max | 0
num = ([max_lt, max_db].max + 1).to_s

db_path = root / 'run/dbs' / (db_preffix + num)
lt_path = root / 'run/tests' / "#{lt_preffix}#{num}.csv"
db_config_path = root / 'run/config' / db_config
lt_config_path = root / 'run/config' / lt_config
db_bin_path = root
lt_bin_path = root / 'load_tester/'

puts "Database config:  #{db_config_path}"
puts "Load test config: #{lt_config_path}"
puts "Database:         #{db_path}"
puts "Load test output: #{lt_path}"
puts ''

# Start KeyVal

`cd #{db_bin_path}; cargo build --profile profiling`

Thread.new do
  samply_cmd = ARGV.include?('-s') ? 'samply record' : ''
  cmd = <<~COMMAND
    #{samply_cmd} ./target/profiling/keyval --database-path #{db_path} #{db_config_path}
  COMMAND

  db_pid = Process.spawn(cmd, chdir: db_bin_path, pgroup: true)

  begin
    Process.wait(db_pid)
  ensure
    Process.kill('-INT', db_pid)
    Process.wait(db_pid)
  end
end

sleep(0.25)
puts("\nDatabase running...")
puts ''

# Run load test

`cd #{lt_bin_path}; cargo run --release -- --output-path #{lt_path} #{lt_config_path}`

sleep(0.5)
puts("\nTest finished...\n")

# Print summary

puts "\n\n"
puts '================='
puts 'Load test results'
puts '================='
puts ''

puts 'Errors:'
duck_db_errors_command = <<~COMMAND
  "SELECT status, AVG(round_trip) as avg_time, COUNT(round_trip) as count
    FROM '#{lt_path}'
    WHERE status != 'Success'
    GROUP BY status"
COMMAND
system("duckdb -c #{duck_db_errors_command}")

puts 'Successful requests:'
duck_db_command = <<~COMMAND
  "SELECT operation, AVG(round_trip) as avg_time,
      approx_quantile(round_trip, 0.50) as p50,
      approx_quantile(round_trip, 0.95) as p95,
      approx_quantile(round_trip, 0.99) as p99
  FROM '#{lt_path}'
  WHERE status = 'Success'
  GROUP BY operation;"
COMMAND
system("duckdb -c #{duck_db_command}")
