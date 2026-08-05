<script setup lang="ts">
import { SpinnerIcon } from '@modrinth/assets'
import { computed, ref, watch } from 'vue'

// Shape of the live `cf_install_progress` payload as consumed by this modal.
export interface CfInstallProgressView {
	phase?: string
	current: number
	total: number
	bytesDownloaded: number
	totalBytes: number
	message?: string
}

const props = defineProps<{
	title: string
	iconUrl: string | null
	progress: CfInstallProgressView | null
}>()

// Rolling sample history for throughput computation. Progress events arrive
// per-file completion (with up to 12 concurrent downloads), so consecutive
// events can be only milliseconds apart — the old last-two-events delta
// produced nonsense "speed" values. Instead keep the most recent ~2s of
// (bytes, time) samples and average the rate across the whole window, which
// converges to the true aggregate download throughput.
const SPEED_WINDOW_MS = 2000
const speedSamples = ref<Array<{ bytes: number; time: number }>>([])

watch(
	() => props.progress,
	(progress) => {
		// Reset the window when the modal closes (progress becomes null) so a
		// fresh install never starts with stale byte samples from the previous
		// one.
		if (!progress) {
			speedSamples.value = []
			return
		}
		const bytes = progress.bytesDownloaded
		if (bytes <= 0) return
		const now = Date.now()
		const samples = [...speedSamples.value, { bytes, time: now }]
		const cutoff = now - SPEED_WINDOW_MS
		while (samples.length > 1 && samples[0].time < cutoff) {
			samples.shift()
		}
		speedSamples.value = samples
	},
)

// Formats bytes to a human-readable string (e.g. "1.2 GB", "142 MB", "24.5 KB").
const formatBytes = (bytes: number): string => {
	if (bytes <= 0) return '0 B'
	const units = ['B', 'KB', 'MB', 'GB', 'TB']
	const i = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1)
	const val = bytes / Math.pow(1024, i)
	return `${val < 10 ? val.toFixed(1) : val.toFixed(0)} ${units[i]}`
}

const percent = computed(() => {
	const p = props.progress
	if (!p || p.total <= 0) return 0
	if (p.totalBytes > 0) {
		return Math.min(100, (p.bytesDownloaded / p.totalBytes) * 100)
	}
	return Math.min(100, (p.current / p.total) * 100)
})

// Computes the current download speed as a human-readable string (e.g.
// "24.5 MB/s") by averaging the bytes downloaded across the last ~2s of
// samples. Only meaningful while files are actively downloading.
const downloadSpeed = computed((): string => {
	const p = props.progress
	if (!p || p.phase !== 'downloading_mods') return ''
	const samples = speedSamples.value
	if (samples.length < 2) return ''
	const first = samples[0]
	const last = samples[samples.length - 1]
	const elapsed = (last.time - first.time) / 1000
	if (elapsed <= 0) return ''
	const bytes = last.bytes - first.bytes
	if (bytes <= 0) return ''
	const bps = bytes / elapsed
	return `${formatBytes(Math.round(bps))}/s`
})

// Subtitle mapped from the live phase payload so it tracks the actual install
// stage instead of always claiming "Downloading mods".
const phaseText = computed(() => {
	switch (props.progress?.phase) {
		case 'fetching_pack':
			return 'Fetching modpack info…'
		case 'downloading_mods':
			return 'Downloading mods'
		case 'installing_minecraft':
			return 'Installing Minecraft and loader'
		case 'finished':
			return 'Finishing…'
		default:
			return 'Installing…'
	}
})

const statusText = computed(() => {
	const p = props.progress
	if (!p) return ''
	const msg = p.message ?? ''
	const parts: string[] = []
	if (p.totalBytes > 0 && p.bytesDownloaded > 0) {
		parts.push(`${formatBytes(p.bytesDownloaded)} / ${formatBytes(p.totalBytes)}`)
	}
	if (downloadSpeed.value) {
		parts.push(downloadSpeed.value)
	}
	const suffix = parts.length > 0 ? ` — ${parts.join(' · ')}` : ''
	return msg + suffix
})
</script>

<template>
	<Teleport to="body">
		<Transition
			enter-active-class="transition-all duration-200 ease-out"
			enter-from-class="opacity-0"
			enter-to-class="opacity-100"
			leave-active-class="transition-all duration-150 ease-in"
			leave-from-class="opacity-100"
			leave-to-class="opacity-0"
		>
			<div
				v-if="progress"
				class="fixed inset-0 z-[100] flex items-center justify-center bg-black/60 backdrop-blur-sm"
			>
				<div
					class="w-[480px] rounded-xl bg-raised p-6 border border-[--brand-gradient-border] shadow-2xl"
				>
					<div class="flex items-center gap-4 mb-4">
						<img v-if="iconUrl" :src="iconUrl" class="w-12 h-12 rounded shrink-0" :alt="title" />
						<div class="min-w-0">
							<h2 class="text-lg font-extrabold m-0 truncate">
								{{ title }}
							</h2>
							<p class="text-sm text-gray-400 m-0 mt-0.5">
								{{ phaseText }}
							</p>
						</div>
						<SpinnerIcon class="animate-spin w-6 h-6 shrink-0 ml-auto text-[--color-brand]" />
					</div>

					<!-- Progress bar -->
					<div class="h-2 w-full overflow-hidden rounded-full bg-[--color-button-bg]">
						<div
							class="h-full rounded-full transition-all duration-200 ease-out"
							:style="{
								width: percent + '%',
								background:
									'linear-gradient(to right, var(--color-brand), var(--color-accent-light))',
							}"
						/>
					</div>

					<!-- Status text -->
					<div class="mt-3 text-center">
						<p class="m-0 text-sm font-semibold text-white/90 truncate" :title="statusText">
							{{ statusText }}
						</p>
						<p class="m-0 mt-1 text-xs text-gray-400">
							{{ progress ? `${progress.current} / ${progress.total} files` : '' }}
						</p>
					</div>
				</div>
			</div>
		</Transition>
	</Teleport>
</template>

<style scoped>
.bg-raised {
	background-color: var(--color-raised-bg);
}
</style>
