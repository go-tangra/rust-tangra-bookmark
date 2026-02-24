import { federation } from '@module-federation/vite';
import vue from '@vitejs/plugin-vue';
import { defineConfig } from 'vite';

export default defineConfig(({ command }) => ({
  // Dev: default '/'; Production: nginx serves bookmark assets at /modules/bookmark/
  base: command === 'serve' ? '/' : '/modules/bookmark/',
  plugins: [
    vue(),
    federation({
      name: 'bookmark',
      filename: 'remoteEntry.js',
      remotes: {
        shell: {
          type: 'module',
          name: 'shell',
          // Dev: shell dev server; Production: same origin as the host app
          entry:
            command === 'serve'
              ? 'http://localhost:5666/remoteEntry.js'
              : '/remoteEntry.js',
        },
      },
      exposes: {
        './module': './src/index.ts',
      },
      shared: {
        vue: { singleton: true },
        'vue-router': { singleton: true },
        pinia: { singleton: true },
        // vue-i18n: handled explicitly via registerModule() + mergeLocaleMessage()
        'ant-design-vue': { singleton: true },
      },
      dts: false,
    }),
  ],
  server: {
    port: 3001,
    strictPort: true,
    origin: 'http://localhost:3001',
    cors: true,
  },
  build: {
    target: 'esnext',
    minify: true,
  },
}));
