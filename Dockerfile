FROM node:24-bookworm-slim AS web
WORKDIR /web
COPY web/package.json web/package-lock.json ./
RUN npm ci
COPY web/ ./
RUN npm run build

FROM rust:1.98-slim-bookworm
WORKDIR /usr/src/app
COPY . .
COPY --from=web /web/dist ./web/dist
ENV RUST_BACKTRACE=1
CMD ["cargo", "test", "--workspace", "--locked"]
