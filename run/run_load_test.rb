# frozen_string_literal: true

require 'pathname'
require 'English'

def fail_with(message)
  warn "\nError: #{message}"
  exit 1
end

def process_exit_description(status)
  status.exitstatus ? "exit status #{status.exitstatus}" : "signal #{status.termsig}"
end

def stop_process_group(pid, signal)
  Process.kill(signal, -pid)
rescue Errno::ESRCH
  nil
ensure
  begin
    Process.wait(pid)
  rescue Errno::ECHILD
    nil
  end
end

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
end
loadtest_outputs = loadtest_outputs.select { |file| file.start_with?(lt_preffix) }

max_db = testdbs.map { |file| file[db_pref_len..].to_i }.max || 0
max_lt = loadtest_outputs.map { |file| file[lt_pref_len..-5].to_i }.max || 0
num = ([max_lt, max_db].max + 1).to_s

db_path = root / 'run/dbs' / (db_preffix + num)
lt_path = root / 'run/tests' / "#{lt_preffix}#{num}.csv"
db_config_path = root / db_config
lt_config_path = root / lt_config
db_bin_path = root
lt_bin_path = root / 'load_tester/'

puts "Database config:  #{db_config_path}"
puts "Load test config: #{lt_config_path}"
puts "Database:         #{db_path}"
puts "Load test output: #{lt_path}"
puts ''

puts 'Building database...'
begin
  build_succeeded = system('cargo', 'build', '--profile', 'profiling', chdir: db_bin_path)
rescue SystemCallError => e
  fail_with("Could not build the database binary: #{e.message}")
end
fail_with("Database build failed (#{process_exit_description($CHILD_STATUS)}).") unless build_succeeded

db_binary_path = db_bin_path / 'target/profiling/keyval'
fail_with("Database build succeeded, but #{db_binary_path} is not executable.") unless File.executable?(db_binary_path)

db_command = [
  db_binary_path.to_s,
  '--database-path',
  db_path.to_s,
  db_config_path.to_s
]
db_command = ['samply', 'record', *db_command] if ARGV.include?('-s')

db_pid = nil
db_running = false
lt_pid = nil
lt_running = false

begin
  begin
    db_pid = Process.spawn(*db_command, chdir: db_bin_path, pgroup: true)
    db_running = true
  rescue SystemCallError => e
    fail_with("Could not start the database server: #{e.message}")
  end

  sleep(0.25)
  db_status = Process.waitpid2(db_pid, Process::WNOHANG)
  if db_status
    db_running = false
    fail_with("Database server stopped during startup (#{process_exit_description(db_status[1])}).")
  end

  puts("\nDatabase running...")
  puts ''

  begin
    lt_pid = Process.spawn(
      'cargo', 'run', '--release', '--', '--output-path', lt_path.to_s, lt_config_path.to_s,
      chdir: lt_bin_path,
      pgroup: true
    )
    lt_running = true
  rescue SystemCallError => e
    fail_with("Could not start the load tester: #{e.message}")
  end

  loop do
    lt_status = Process.waitpid2(lt_pid, Process::WNOHANG)
    if lt_status
      lt_running = false
      fail_with("Load test failed (#{process_exit_description(lt_status[1])}).") unless lt_status[1].success?
      break
    end

    db_status = Process.waitpid2(db_pid, Process::WNOHANG)
    if db_status
      db_running = false
      stop_process_group(lt_pid, 'TERM')
      lt_running = false
      fail_with("Database server stopped while the load test was running (#{process_exit_description(db_status[1])}).")
    end

    sleep(0.1)
  end
ensure
  stop_process_group(lt_pid, 'TERM') if lt_running
  stop_process_group(db_pid, 'INT') if db_running
end

puts("\nTest finished...\n")

# Print summary

puts "\n\n"
puts '================='
puts 'Load test results'
puts '================='
puts ''

puts 'Errors:'
sql_lt_path = lt_path.to_s.gsub("'", "''")
duck_db_errors_command = <<~SQL
  SELECT status, AVG(round_trip) as avg_time, COUNT(round_trip) as count
    FROM read_csv('#{sql_lt_path}', comment = '#')
    WHERE status != 'Success'
    GROUP BY status
SQL
begin
  errors_summarized = system('duckdb', '-c', duck_db_errors_command)
rescue SystemCallError => e
  fail_with("Could not summarize load-test errors with duckdb: #{e.message}")
end
unless errors_summarized
  fail_with("Could not summarize load-test errors with duckdb (#{process_exit_description($CHILD_STATUS)}).")
end

puts 'Successful requests:'
duck_db_command = <<~SQL
  SELECT operation, AVG(round_trip) as avg_time,
      approx_quantile(round_trip, 0.50) as p50,
      approx_quantile(round_trip, 0.95) as p95,
      approx_quantile(round_trip, 0.99) as p99
  FROM read_csv('#{sql_lt_path}', comment = '#')
  WHERE status = 'Success'
  GROUP BY operation;
SQL
begin
  requests_summarized = system('duckdb', '-c', duck_db_command)
rescue SystemCallError => e
  fail_with("Could not summarize successful requests with duckdb: #{e.message}")
end
unless requests_summarized
  fail_with("Could not summarize successful requests with duckdb (#{process_exit_description($CHILD_STATUS)}).")
end

throughput_line = File.foreach(lt_path).drop(1).first
fail_with("Load-test output #{lt_path} is missing the throughput row.") unless throughput_line
fail_with("Load-test output #{lt_path} has a malformed throughput row.") unless throughput_line.start_with?('#')

vals = throughput_line[1..].split(',').map(&:strip)
unless vals.length >= 3 && vals.first(3).all? { |value| value.match?(/\A\d+\z/) }
  fail_with("Load-test output #{lt_path} has a malformed throughput row.")
end

time, reads, writes = vals.first(3).map(&:to_i)

puts ''
puts "Reads: #{reads}, writes: #{writes}, total: #{reads + writes}"
puts "Total time: #{format('%.2f', time.to_f / 1000)} ms"
puts "Throughput: #{format('%.2f', 1000.0 * (reads + writes) / time)} ops/ms"
puts ''
