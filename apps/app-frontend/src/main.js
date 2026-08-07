import 'floating-vue/dist/style.css'
import 'overlayscrollbars/overlayscrollbars.css'

import * as Sentry from '@sentry/vue'
import { VueScanPlugin } from '@taijased/vue-render-tracker'
import { VueQueryPlugin } from '@tanstack/vue-query'
import Tres from '@tresjs/core'
import FloatingVue from 'floating-vue'
import { createPinia } from 'pinia'
import { createApp } from 'vue'

import App from '@/App.vue'
import { overlayScrollbarsDirective } from '@/directives/overlayScrollbars'
import i18nPlugin from '@/plugins/i18n'
import i18nDebugPlugin from '@/plugins/i18n-debug'
import router from '@/routes'
import { useError } from '@/store/error.js'

const vueScan = new VueScanPlugin({
	enabled: false, // Enable or disable the tracker
	showOverlay: true, // Show overlay to visualize renders
	log: false, // Log render events to the console
	playSound: false, // Play sound on each render
})

const pinia = createPinia()

let app = createApp(App)

// Suppress known benign TresJS v4.3.x warnings (GitHub: tresjs/tres#905).
// TresCanvas internally calls provide() inside onMounted() rather than during
// synchronous setup(), which triggers Vue warnings. This is a library-level bug
// that cannot be fixed from application code; it does not affect rendering.
const TRES_WARN_PATTERNS = [
	'provide() can only be used inside setup()',
	'onUnmounted is called when there is no active component instance',
]
app.config.warnHandler = (msg, instance, trace) => {
	if (TRES_WARN_PATTERNS.some((pattern) => msg.includes(pattern))) return
	console.warn(`[Vue warn]: ${msg}${trace}`)
}

Sentry.init({
	app,
	dsn: 'https://03f2ad671fafdadbe2a4c11ae884f4c5@o4508388109451264.ingest.de.sentry.io/4508609682014288',
	integrations: [Sentry.browserTracingIntegration({ router })],
	tracesSampleRate: 0.1,
})

app.use(Tres)
app.use(VueQueryPlugin)
app.use(vueScan)
app.use(router)
app.use(pinia)
app.use(FloatingVue, {
	themes: {
		'ribbit-popout': {
			$extend: 'dropdown',
			placement: 'bottom-end',
			instantMove: true,
			distance: 8,
		},
		'dismissable-prompt': {
			$extend: 'dropdown',
			placement: 'bottom-start',
		},
	},
})
app.use(i18nPlugin)
app.use(i18nDebugPlugin)
app.directive('overlay-scrollbars', overlayScrollbarsDirective)

router.isReady().then(() => {
	app.mount('#app')
})

window.addEventListener('unhandledrejection', (event) => {
	console.error('Unhandled promise rejection:', event.reason)
	const error = useError()
	error.showError(event.reason, 'Unhandled Promise Rejection')
})

document.addEventListener('contextmenu', (event) => {
	event.preventDefault()
})
