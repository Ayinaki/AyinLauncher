import path from 'node:path'

import { templateCompilerOptions } from '@tresjs/core'
import vue from '@vitejs/plugin-vue'
import { defineConfig } from 'vite'
import svgLoader from 'vite-svg-loader'

export default defineConfig({
	plugins: [
		vue({
			...templateCompilerOptions,
		}),
		svgLoader({
			svgoConfig: {
				plugins: [
					{
						name: 'preset-default',
						params: {
							overrides: {
								removeViewBox: false,
								cleanupIds: {
									minify: false,
								},
							},
						},
					},
				],
			},
		}),
	],
	cacheDir: '.vite',

	resolve: {
		alias: {
			'@': path.resolve(__dirname, 'src'),
			'#ui': path.resolve(__dirname, 'src'),
		},
	},

	build: {
		lib: {
			entry: path.resolve(__dirname, 'index.ts'),
			name: 'ModrinthUI',
			formats: ['es'],
			fileName: 'index',
		},
		rollupOptions: {
			external: ['vue'],
		},
	},
})
