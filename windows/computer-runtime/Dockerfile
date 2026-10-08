FROM mcr.microsoft.com/playwright:v1.63.0-noble
WORKDIR /opt/roadeep
COPY package.json ./
RUN npm install --omit=dev --ignore-scripts --no-audit --no-fund && \
    mkdir -p /workspace && chown 1000:1000 /workspace
COPY policy.mjs server.mjs client.mjs ./
ENV PLAYWRIGHT_BROWSERS_PATH=/ms-playwright HOME=/tmp/home
USER 1000:1000
WORKDIR /workspace
CMD ["node", "/opt/roadeep/server.mjs"]
