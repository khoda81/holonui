# Use the official Rust image
FROM rust:1.77-bookworm as builder

WORKDIR /usr/src/holon_app

# Install dependencies required by eframe/egui for Linux
RUN apt-get update && apt-get install -y \
    libxcb-render0-dev \
    libxcb-shape0-dev \
    libxcb-xfixes0-dev \
    libxkbcommon-dev \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Copy your source code
COPY . .

# Run phase
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y libxcb-render0 libxcb-shape0 libxcb-xfixes0 libxkbcommon0 ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /usr/src/holon_app/target/release/holon_app /usr/local/bin/holon_app
COPY .env /.env

CMD ["trunk serve"]
