# Period jitter over time.
#
# Render:
#   gnuplot -e "data='run.ndjson'" jitter-trace.gp
# Produces jitter-trace.png. Requires gnuplot and jq.
#
# The `< jq ...` pipe projects NDJSON into two whitespace columns
# (ts_ns jitter_ns), skipping cycles with null jitter (first cycle / faulted).

set terminal pngcairo size 1200,600 font ",11"
set output 'jitter-trace.png'
set title 'Executor period jitter over time (idle profile)'
set xlabel 'time since first sample (ms)'
set ylabel 'period jitter (us)'
set grid

t0 = NaN
plot "< jq -r 'select(.jitter_ns!=null) | \"\\(.ts_ns) \\(.jitter_ns)\"' ".data \
     using (t0 = (t0!=t0 ? $1 : t0), ($1-t0)/1e6):($2/1e3) \
     with linespoints pt 7 ps 0.3 lc rgb '#1f77b4' title 'period jitter'
