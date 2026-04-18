# --- Stage 1: Build the WASM app using Trunk ---
FROM rust:1.77-slim AS builder

# Install WASM target and Trunk (WASM bundler for Rust)
RUN rustup target add wasm32-unknown-unknown
RUN cargo install trunk

WORKDIR /app
COPY . .

# Build the release-optimized WASM bundle
# This creates a 'dist' folder containing the .wasm, .js, and index.html
RUN trunk build --release

# --- Stage 2: Serve the static files using Nginx ---
FROM nginx:alpine

# Copy the build artifacts from the builder stage to Nginx's html directory
COPY --from=builder /app/dist /usr/share/nginx/html

# Expose port 80
EXPOSE 80

# Start Nginx
CMD ["nginx", "-g", "daemon off;"]
