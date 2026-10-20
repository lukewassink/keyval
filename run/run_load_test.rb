# frozen_string_literal: true

require 'pathname'

puts "\nSetting up load test..."
puts ''

# File names
db_config = 'db_config.toml'
lt_config = 'lt_config.toml'
db_preffix = 'testdb_'
lt_preffix = 'loadtest_'

# Compute file paths
db_pref_len = db_preffix.length
lt_pref_len = lt_preffix.length

root = Pathname.new(__dir__)
children = root.children.map { |c| c.basename.to_s }
testdbs = children.select { |file| file.start_with?(db_preffix) }
loadtest_outputs = children.select { |file| file.start_with?(lt_preffix) }
max_db = testdbs.map { |file| file[db_pref_len..].to_i }.max
max_lt = loadtest_outputs.map { |file| file[lt_pref_len..-5].to_i }.max
num = ([max_lt, max_db].max + 1).to_s

db_path = root / (db_preffix + num)
lt_path = root / "#{lt_preffix}#{num}.csv"
db_config_path = root / db_config
lt_config_path = root / lt_config
db_bin_path = root / '..'
lt_bin_path = root / '../load_tester/'

puts "Database config:  #{db_config_path}"
puts "Load test config: #{lt_config_path}"
puts "Database:         #{db_path}"
puts "Load test output: #{lt_path}"

# Start KeyVal

puts("\nBuilding database...")
puts ''
Thread.new do
  db_pid = Process.spawn("cd #{db_bin_path}; cargo run -- --database-path #{db_path} #{db_config_path}", pgroup: true)
  begin
    Process.wait(db_pid)
  ensure
    Process.kill('-TERM', db_pid)
  end
end

sleep(0.5)
puts('\nDatabase running...\n')

# Run load test

`cd #{lt_bin_path}; cargo run -- --output-path #{lt_path} #{lt_config_path}`

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
  "SELECT status, AVG(round_trip)
  FROM '#{lt_path}'
  WHERE status != 'Success'
  GROUP BY status"
COMMAND
system("duckdb -c #{duck_db_errors_command}")

puts 'Successful requests:'
duck_db_command = <<~COMMAND
  "SELECT operation, AVG(round_trip)
  FROM '#{lt_path}'
  WHERE status = 'Success'
  GROUP BY operation;"
COMMAND
system("duckdb -c #{duck_db_command}")
