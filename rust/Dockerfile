###############################################################################
# Multi-stage Dockerfile that builds:
#   1. Ragel 7 (from colm-suite) — generates lexer.rs from lexer.rl
#   2. Bison + rust-bison-skeleton — generates parser.rs from parser.y
#   3. cargo build — compiles the Rust crate
#
# The generated .rs files are placed at /out/ in the final stage.
#
# Usage:
#   docker build -t rulekit-rust .
#   docker run --rm rulekit-rust
#   # Extract generated code:
#   id=$(docker create rulekit-rust) && docker cp $id:/out/ ./gen && docker rm $id
###############################################################################

# ---------- stage 1: build colm-suite (colm + ragel 7) ----------
FROM debian:bookworm-slim AS ragel-builder

RUN apt-get update && apt-get install -y --no-install-recommends \
        build-essential autoconf automake libtool git ca-certificates pkg-config \
    && rm -rf /var/lib/apt/lists/*

# colm-suite is a unified repo: top-level build produces both colm and ragel
RUN git clone --depth 1 https://github.com/adrian-thurston/colm-suite.git /tmp/colm-suite \
    && cd /tmp/colm-suite \
    && ./autogen.sh \
    && ./configure --prefix=/usr/local \
    && make -j"$(nproc)" \
    && make install

# ---------- stage 2: generate code & build ----------
FROM debian:bookworm-slim AS builder

RUN apt-get update && apt-get install -y --no-install-recommends \
        build-essential bison curl ca-certificates pkg-config \
    && rm -rf /var/lib/apt/lists/*

# Copy colm-suite binaries and libraries from stage 1
COPY --from=ragel-builder /usr/local/bin/ /usr/local/bin/
COPY --from=ragel-builder /usr/local/lib/ /usr/local/lib/
COPY --from=ragel-builder /usr/local/share/ /usr/local/share/
RUN ldconfig

# Install Rust via rustup
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
ENV PATH="/root/.cargo/bin:${PATH}"

WORKDIR /build
COPY . .

# Step 1: Generate lexer.rs from lexer.rl using ragel-rust (Ragel 7)
# Post-process to fix cast-before-index codegen bug in Ragel 7's Rust backend
RUN ragel-rust -o src/lexer.rs src/lexer.rl \
    && perl -i -pe 's/\( (_rule_lexer_\w+) \) as (\w+)\[([^\]]+)\]/$1\[$3\] as $2/g' src/lexer.rs \
    && perl -i -pe 's/\] as u32/\] as i32/g; s/as u32;/as i32;/g' src/lexer.rs \
    && echo "--- lexer.rs generated ---"

# Step 2: cargo build triggers build.rs which runs bison for parser.y
RUN cargo build 2>&1 && echo "--- build complete ---"

# Copy generated files to /out/
RUN mkdir -p /out \
    && cp src/lexer.rs /out/lexer.rs \
    && cp src/parser.rs /out/parser.rs \
    && ls -la /out/

# ---------- stage 3: minimal output image ----------
FROM debian:bookworm-slim
COPY --from=builder /out/ /out/
COPY --from=builder /build/target/debug/rulekit /usr/local/bin/rulekit
CMD ["sh", "-c", "echo '--- Generated files in /out/ ---' && ls -la /out/"]
