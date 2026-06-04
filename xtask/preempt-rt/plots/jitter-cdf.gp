# Cumulative distribution of period jitter (the p50/p95/p99/max story).
#
# Render:
#   gnuplot -e "data='run.ndjson'" jitter-cdf.gp
# Produces jitter-cdf.png. Requires gnuplot and jq.
#
# Log x-axis: jitter spans sub-microsecond to multi-millisecond, so a linear
# axis crushes the body against the left edge when a single outlier stretches
# the range. The `< jq ... | sort -n | awk ...` pipe emits "jitter_us frac"
# pairs (rank/N) -- a deterministic empirical CDF. Zeros are floored to 0.1 us
# so they stay visible on the log axis (they appear as the first riser).

set terminal pngcairo size 1000,600 font ",11"
set output 'jitter-cdf.png'
set title 'Executor period jitter CDF (idle profile)'
set xlabel 'period jitter (us, log scale)'
set ylabel 'cumulative fraction'
set logscale x
set xrange [0.1:*]
set yrange [0:1]
set grid
set key bottom right

plot "< jq -r 'select(.jitter_ns!=null)|.jitter_ns' ".data." | sort -n | awk '{v[NR]=$1} END{for(i=1;i<=NR;i++){x=v[i]/1000.0; if(x<0.1)x=0.1; print x, i/NR}}'" \
     using 1:2 with steps lw 2 lc rgb '#d62728' title 'CDF'
