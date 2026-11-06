# shellcheck shell=bash disable=SC2034
# Experiment settings. Override any value by exporting it before a run, e.g.
#   DURATION=30 bench/run.sh B1-eevdf 50000 1

# memcached (server)
MC_PORT="${MC_PORT:-11211}"
MC_THREADS="${MC_THREADS:-4}"
MC_MEMORY_MB="${MC_MEMORY_MB:-4096}"

# memtier_benchmark (client). Total connections = MT_THREADS * MT_CONNS.
MT_THREADS="${MT_THREADS:-4}"
MT_CONNS="${MT_CONNS:-25}"
MT_KEYS="${MT_KEYS:-100000}"
MT_VALUE_BYTES="${MT_VALUE_BYTES:-64}"
MT_RATIO="${MT_RATIO:-1:10}"            # set:get

# Run shape
DURATION="${DURATION:-60}"               # seconds of measured load
SETTLE="${SETTLE:-5}"                    # seconds of batch load before measuring

# Batch jobs: any of "kbuild ffmpeg"
BATCH="${BATCH:-kbuild ffmpeg}"
BATCH_JOBS="${BATCH_JOBS:-$(nproc)}"     # make -j

# Batch CPU set for B2-cgroup and B5-fixedk: the highest-numbered BATCH_K CPUs.
BATCH_K="${BATCH_K:-$(( $(nproc) / 2 ))}"
batch_cpuset() { local n; n=$(nproc); echo "$(( n - BATCH_K ))-$(( n - 1 ))"; }

# Variants (docs/TAXONOMY.md section 4)
ALL_VARIANTS="B0-alone B1-eevdf B2-cgroup B3-rustland B4-partial B5-fixedk"
