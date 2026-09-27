 FROM rust:latest

# System dependencies required for Rust native/C++ crates
RUN apt-get update && apt-get install -y \
    curl \
    ca-certificates \
    git \
    cmake \
    build-essential \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Install Node.js 22
RUN curl -fsSL https://deb.nodesource.com/setup_22.x | bash - \
    && apt-get install -y nodejs \
    && rm -rf /var/lib/apt/lists/*

# Verify installed tools
RUN node --version
RUN npm --version
RUN rustc --version
RUN cargo --version
RUN cmake --version
RUN gcc --version
RUN g++ --version

WORKDIR /workspace

CMD ["sh"]