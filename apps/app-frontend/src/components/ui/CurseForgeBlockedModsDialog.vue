<script setup lang="ts">
import { injectNotificationManager } from '@modrinth/ui'
import { openUrl } from '@tauri-apps/plugin-opener'
import { computed, ref, watch } from 'vue'

import {
	type BlockedModScanResult,
	type CurseForgeBlockedMod,
	scan_downloads_for_blocked_mods,
} from '@/helpers/install'

const props = defineProps<{
	open: boolean
	packTitle: string
	instanceId: string
	mods: CurseForgeBlockedMod[]
}>()

const emit = defineEmits<{ (e: 'close'): void }>()

const { handleError } = injectNotificationManager()

type ScanStatus = 'pending' | 'moved' | 'not_found' | 'conflict'
type ScanStatusEntry = { status: ScanStatus; destination: string | null }

const scanStatuses = ref<Record<number, ScanStatusEntry>>({})
const openingAll = ref(false)
const scanningDownloads = ref(false)
const scanMessage = ref<string | null>(null)

// Reset per-item scan statuses whenever a new set of blocked mods is shown
// (fresh install, update, or version change).
watch(
	() => props.mods,
	(mods) => {
		const statuses: Record<number, ScanStatusEntry> = {}
		for (const mod of mods) {
			statuses[mod.fileId] = { status: 'pending', destination: null }
		}
		scanStatuses.value = statuses
		scanMessage.value = null
	},
	{ immediate: true },
)

// Mods that still need a manual download (pending, or not found after a scan).
const unresolvedScanMods = computed(() =>
	props.mods.filter((mod) => {
		const s = scanStatuses.value[mod.fileId]
		return !s || s.status === 'pending' || s.status === 'not_found'
	}),
)

// Opens every blocked mod's CurseForge page in the default browser, with a
// small delay between each to avoid the OS/browser flagging it as popup spam.
const openAllLinks = async () => {
	if (openingAll.value || unresolvedScanMods.value.length === 0) return
	openingAll.value = true
	try {
		for (const mod of unresolvedScanMods.value) {
			// Continue with the remaining links if one URL fails.
			await openUrl(mod.websiteUrl).catch(() => {})
			await new Promise((resolve) => setTimeout(resolve, 500))
		}
	} finally {
		openingAll.value = false
	}
}

// Scans the user's OS Downloads folder for files matching the blocked mods'
// expected filenames and moves the matches into the instance's mods folder.
// Only mods that are still pending/not_found are scanned each time, so
// already-moved mods are never re-scanned and their statuses are preserved.
const scanDownloadsAndMove = async () => {
	if (scanningDownloads.value || props.mods.length === 0) return
	const unresolved = unresolvedScanMods.value
	if (unresolved.length === 0) {
		scanMessage.value = 'All mods have already been moved.'
		return
	}
	scanningDownloads.value = true
	scanMessage.value = null
	try {
		const result = await scan_downloads_for_blocked_mods(props.instanceId, unresolved)
		// Record the per-item outcome so each row shows moved/not-found/conflict.
		// Already-moved entries keep their existing status (they weren't in the
		// scan request, so they won't appear in result.items).
		const statuses = { ...scanStatuses.value }
		for (const item of result.items) {
			statuses[item.fileId] = { status: item.status, destination: item.destination }
		}
		scanStatuses.value = statuses
		scanMessage.value = buildScanSummary(result)
		// All entries stay in the list so their per-item status badges are
		// visible; unresolved ones keep their download links.
	} catch (error) {
		handleError(error)
	} finally {
		scanningDownloads.value = false
	}
}

// Summarizes a scan result: how many files were moved and into which folders,
// plus how many were replaced and how many still couldn't be found.
const buildScanSummary = (result: BlockedModScanResult): string => {
	if (result.moved === 0) return 'No matching files found in your Downloads folder.'
	const byDest: Record<string, number> = {}
	let conflicts = 0
	let notFound = 0
	for (const item of result.items) {
		if (item.status === 'not_found') {
			notFound += 1
			continue
		}
		const key = item.destination ?? 'mods'
		byDest[key] = (byDest[key] ?? 0) + 1
		if (item.status === 'conflict') conflicts += 1
	}
	const parts = Object.entries(byDest).map(([dir, count]) => `${dir}/: ${count}`)
	let summary = `Moved ${result.moved} file(s) — ${parts.join(', ')}`
	if (conflicts > 0) summary += ` (${conflicts} replaced an existing file)`
	if (notFound > 0) summary += `. ${notFound} still not found`
	return summary
}

// Badge styling per scan status.
const scanStatusClass = (status: ScanStatus) => {
	switch (status) {
		case 'moved':
			return 'bg-emerald-500/20 text-emerald-300'
		case 'conflict':
			return 'bg-amber-500/20 text-amber-300'
		case 'not_found':
			return 'bg-red-500/20 text-red-300'
		default:
			return 'bg-white/10 text-gray-300'
	}
}

// Human-readable badge label per scan status.
const scanStatusLabel = (entry: ScanStatusEntry) => {
	switch (entry.status) {
		case 'moved':
			return `Moved to ${entry.destination ?? 'mods'}/`
		case 'conflict':
			return `Replaced in ${entry.destination ?? 'mods'}/`
		case 'not_found':
			return 'Not found'
		default:
			return 'Pending'
	}
}
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
				v-if="open"
				class="fixed inset-0 z-[100] flex items-center justify-center bg-black/60 backdrop-blur-sm"
				@click.self="emit('close')"
			>
				<div
					class="w-[560px] max-h-[80vh] overflow-y-auto rounded-xl bg-raised p-6 border border-[--brand-gradient-border] shadow-2xl"
				>
					<div class="flex items-start justify-between gap-4 mb-4">
						<div>
							<h2 class="text-xl font-extrabold m-0">Manual downloads required</h2>
							<p class="text-sm text-gray-400 m-0 mt-1">
								{{ mods.length }} mod(s) in {{ packTitle }} disallow automated downloads and were
								skipped. Download them manually, then scan your Downloads folder to move them into
								the instance.
							</p>
						</div>
						<button
							class="shrink-0 rounded-lg p-2 text-gray-400 transition-colors duration-150 hover:bg-[--color-button-bg] hover:text-white"
							@click="emit('close')"
						>
							<svg class="w-5 h-5" viewBox="0 0 24 24" fill="none" stroke="currentColor">
								<path
									stroke-linecap="round"
									stroke-linejoin="round"
									stroke-width="2"
									d="M6 18L18 6M6 6l12 12"
								/>
							</svg>
						</button>
					</div>

					<div v-if="unresolvedScanMods.length > 0" class="flex gap-2 mb-3">
						<button
							class="flex-1 rounded-lg bg-[--color-button-bg] px-3 py-2 text-sm font-bold text-white transition-transform duration-150 hover:brightness-110 active:scale-[0.98] disabled:opacity-50 disabled:cursor-not-allowed"
							:disabled="openingAll"
							@click="openAllLinks"
						>
							{{ openingAll ? 'Opening…' : `Open All (${unresolvedScanMods.length})` }}
						</button>
						<button
							class="flex-1 rounded-lg bg-[--brand-gradient-from] px-3 py-2 text-sm font-bold text-white transition-transform duration-150 hover:brightness-110 active:scale-[0.98] disabled:opacity-50 disabled:cursor-not-allowed"
							:disabled="scanningDownloads"
							@click="scanDownloadsAndMove"
						>
							{{ scanningDownloads ? 'Scanning…' : 'Scan Downloads & Move' }}
						</button>
					</div>

					<p
						v-if="scanMessage"
						class="m-0 mb-3 rounded-lg bg-[--color-button-bg] px-3 py-2 text-sm font-semibold text-white"
					>
						{{ scanMessage }}
					</p>

					<ul class="flex flex-col gap-2 m-0 p-0 list-none">
						<li
							v-for="mod in mods"
							:key="`${mod.projectId}-${mod.fileId}`"
							class="flex items-center justify-between gap-3 rounded-lg bg-[--color-button-bg] px-4 py-3"
							:class="{
								'opacity-60':
									scanStatuses[mod.fileId]?.status === 'moved' ||
									scanStatuses[mod.fileId]?.status === 'conflict',
							}"
						>
							<div class="min-w-0">
								<p class="m-0 truncate font-bold">{{ mod.name }}</p>
								<p class="m-0 truncate text-xs text-gray-400">
									Project {{ mod.projectId }} · File {{ mod.fileId }}
								</p>
							</div>
							<div class="flex items-center gap-2 shrink-0">
								<span
									v-if="scanStatuses[mod.fileId]"
									class="shrink-0 rounded-full px-2.5 py-1 text-xs font-bold"
									:class="scanStatusClass(scanStatuses[mod.fileId].status)"
								>
									{{ scanStatusLabel(scanStatuses[mod.fileId]) }}
								</span>
								<button
									v-if="
										scanStatuses[mod.fileId]?.status !== 'moved' &&
										scanStatuses[mod.fileId]?.status !== 'conflict'
									"
									class="shrink-0 rounded-lg bg-[--brand-gradient-from] px-3 py-1.5 text-sm font-bold text-white transition-transform duration-150 active:scale-95"
									@click="openUrl(mod.websiteUrl)"
								>
									Download from CurseForge
								</button>
							</div>
						</li>
					</ul>

					<button
						class="mt-4 w-full rounded-lg bg-[--color-button-bg] px-4 py-2 font-bold text-white transition-transform duration-150 active:scale-[0.98]"
						@click="emit('close')"
					>
						Done
					</button>
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
