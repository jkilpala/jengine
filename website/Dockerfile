# Stage 1: Build static site
FROM node:20-alpine AS builder

# Install cargo/rustup for rustdoc generation (optional — remove if rustdoc not needed in Docker)
# RUN apk add --no-cache curl build-base && \
#     curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
# ENV PATH="/root/.cargo/bin:${PATH}"

WORKDIR /repo
COPY . .

WORKDIR /app
COPY website/package*.json ./
RUN npm ci --production=false

COPY website/ .

# Point sync scripts to the repo root (one level up)
ENV JENGINE_ROOT=/repo

# Run sync scripts and build
RUN npm run build

# Stage 2: Serve with nginx
FROM nginx:alpine

# Security: run as non-root
RUN addgroup -g 1001 -S appgroup && \
    adduser -u 1001 -S appuser -G appgroup

COPY --from=builder /app/dist /usr/share/nginx/html
COPY website/nginx.conf /etc/nginx/conf.d/default.conf

# Create required nginx dirs with proper permissions
RUN chown -R appuser:appgroup /usr/share/nginx/html && \
    chown -R appuser:appgroup /var/cache/nginx && \
    chown -R appuser:appgroup /var/log/nginx && \
    touch /var/run/nginx.pid && \
    chown appuser:appgroup /var/run/nginx.pid

EXPOSE 80

CMD ["nginx", "-g", "daemon off;"]
