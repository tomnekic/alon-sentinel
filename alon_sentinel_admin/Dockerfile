# ── Build ────────────────────────────────────────────────────────────────────
FROM node:22-alpine AS builder
WORKDIR /app

COPY package.json package-lock.json ./
RUN npm ci

COPY . .

ARG VITE_SHOW_DEMO_ACCOUNT=false
ENV VITE_SHOW_DEMO_ACCOUNT=$VITE_SHOW_DEMO_ACCOUNT

RUN npm run build

# ── Runtime ──────────────────────────────────────────────────────────────────
FROM nginx:1.27-alpine AS runtime

COPY --from=builder /app/dist     /usr/share/nginx/html
COPY nginx.conf                   /etc/nginx/conf.d/default.conf
