import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import { readFile, rm, writeFile } from 'node:fs/promises'
import { fileURLToPath } from 'node:url'

const hotFile = fileURLToPath(new URL('./hot', import.meta.url))

function locoHotFile() {
  let publishedUrl

  return {
    name: 'loco-inertia-hot-file',
    configureServer(server) {
      const httpServer = server.httpServer
      if (!httpServer) return

      httpServer.once('listening', () => {
        // Vite assigns resolvedUrls immediately after the listening event completes.
        setImmediate(async () => {
          publishedUrl = server.resolvedUrls?.local[0] ?? server.resolvedUrls?.network[0]
          if (!publishedUrl) return

          try {
            await writeFile(hotFile, `${publishedUrl}\n`, 'utf8')
            console.info(`Loco hot file: ${hotFile} -> ${publishedUrl}`)
          } catch (error) {
            console.error(`Could not write Loco hot file: ${error}`)
          }
        })
      })

      httpServer.once('close', () => {
        void (async () => {
          try {
            const currentUrl = (await readFile(hotFile, 'utf8')).trim()
            if (currentUrl === publishedUrl) await rm(hotFile, { force: true })
          } catch (error) {
            if (error.code !== 'ENOENT') console.error(`Could not remove Loco hot file: ${error}`)
          }
        })()
      })
    },
  }
}

export default defineConfig(({ isSsrBuild }) => ({
  plugins: [react(), locoHotFile()],
  server: {
    host: '127.0.0.1',
    port: 5173,
  },
  build: {
    outDir: isSsrBuild ? 'ssr-dist' : '../static/assets',
    emptyOutDir: true,
    rollupOptions: {
      input: isSsrBuild ? 'ssr.jsx' : 'app.jsx',
      output: {
        entryFileNames: isSsrBuild ? 'ssr.js' : 'app.js',
        assetFileNames: '[name][extname]',
      },
    },
  },
}))

