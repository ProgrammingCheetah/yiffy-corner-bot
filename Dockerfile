FROM node:18-alpine

COPY package*.json ./

RUN npm ci --omit=dev && npm cache clean --force
COPY . . 
RUN npm run build
ENV NODE_ENV production

CMD ["sh", "-c", "node dist/index.js"]