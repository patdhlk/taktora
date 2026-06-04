# Cumulative distribution of period jitter (the p50/p95/p99/max story).
#
# Render:
#   gnuplot -e "data='run.ndjson'" jitter-cdf.gp
# Produces jitter-cdf.png. Requires gnuplot and jq.

set terminal pngcairo size 1000,600 font ",11"
set output 'jitter-cdf.png'
set title 'Executor period jitter CDF (idle profile)'
set xlabel 'period jitter (us)'
set ylabel 'cumulative fraction'
set yrange [0:1]
set grid

# First pass: count samples so we can normalize the cumulative count.
stats "< jq -r 'select(.jitter_ns!=null) | .jitter_ns' ".data nooutput
N = STATS_records

# `smooth cumulative` sums y as x increases; y=1/N per sample => fraction.
plot "< jq -r 'select(.jitter_ns!=null) | .jitter_ns' ".data." | sort -n" \
     using ($1/1e3):(1.0/N) smooth cumulative with lines lw 2 lc rgb '#d62728' \
     title 'CDF'
