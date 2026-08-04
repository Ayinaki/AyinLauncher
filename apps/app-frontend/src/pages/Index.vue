<script setup lang="ts">
import { DownloadIcon, PlayIcon, SpinnerIcon } from '@modrinth/assets'
import { ButtonStyled, injectNotificationManager } from '@modrinth/ui'
import { openUrl } from '@tauri-apps/plugin-opener'
import dayjs from 'dayjs'
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import { useRoute } from 'vue-router'

import curseforgePacks from '@/assets/curseforge-packs.json'
import RecentWorldsList from '@/components/ui/world/RecentWorldsList.vue'
import { trackEvent } from '@/helpers/analytics'
import { get_project, get_search_results, get_version_many } from '@/helpers/cache.js'
import { cf_install_progress_listener, instance_listener, process_listener } from '@/helpers/events'
import {
	type BlockedModScanResult,
	check_curseforge_pack_update,
	type CurseForgeBlockedMod,
	type CurseForgeCatalogPack,
	get_curseforge_catalog,
	install_curseforge_catalog_pack,
	scan_downloads_for_blocked_mods,
} from '@/helpers/install'
import { list, run, update_managed_modrinth_version } from '@/helpers/instance'
import type { GameInstance } from '@/helpers/types'
import { injectContentInstall } from '@/providers/content-install'
import { useBreadcrumbs } from '@/store/breadcrumbs'

const { handleError } = injectNotificationManager()
const { install: installVersion } = injectContentInstall()

const featuredModpacks = ref([])
const featuredMods = ref([])
const filter = ref('')

const route = useRoute()
const breadcrumbs = useBreadcrumbs()

breadcrumbs.setRootContext({ name: 'Home', link: route.path })

const recentInstances = ref<GameInstance[]>([])

const offline = ref(!navigator.onLine)
window.addEventListener('offline', () => {
	offline.value = true
})
window.addEventListener('online', () => {
	offline.value = false
})

const getInstances = async () => {
	const instances = (await list().catch(handleError)) ?? []

	recentInstances.value = instances
		.filter((x) => x.last_played)
		.sort((a, b) => {
			const dateA = dayjs(a.last_played)
			const dateB = dayjs(b.last_played)

			if (dateA.isSame(dateB)) {
				return a.name.localeCompare(b.name)
			}

			return dateB - dateA
		})

	filter.value = ''
}

const installing = ref({})
const installed = ref({})
const updating = ref({})
const hasUpdate = ref({})

const installProgress = ref<Record<string, {
	phase?: string
	current: number
	total: number
	bytesDownloaded: number
	totalBytes: number
	message?: string
}>>({})
const blockedModsDialog = ref<{
	packTitle: string
	instanceId: string
	mods: CurseForgeBlockedMod[]
} | null>(null)
const openingAll = ref(false)
const scanningDownloads = ref(false)
const scanMessage = ref<string | null>(null)
type ScanStatus = 'pending' | 'moved' | 'not_found' | 'conflict'
type ScanStatusEntry = { status: ScanStatus; destination: string | null }
const scanStatuses = ref<Record<number, ScanStatusEntry>>({})
// Mods that still need a manual download (pending, or not found after a scan).
const unresolvedScanMods = computed(() => {
	const dialog = blockedModsDialog.value
	if (!dialog) return []
	return dialog.mods.filter((mod) => {
		const s = scanStatuses.value[mod.fileId]
		return !s || s.status === 'pending' || s.status === 'not_found'
	})
})

const selectedModpackId = ref(localStorage.getItem('lastSelectedModpack') || '')
const selectedModpack = computed(() =>
	featuredModpacks.value.find((m) => m.project_id === selectedModpackId.value),
)

const featuredProjects = ref([])

const FEATURED_COLLECTION_ID = 'jxEdftkP'

const fetchFeaturedProjects = async () => {
	try {
		const response = await fetch(`https://api.modrinth.com/v3/collection/${FEATURED_COLLECTION_ID}`)
		const data = await response.json()
		featuredProjects.value = data.projects.map((id) => ({ id }))
		return true
	} catch (error) {
		handleError(error)
		featuredProjects.value = []
		return false
	}
}

const getFeaturedModpacks = async () => {
	await fetchFeaturedProjects()
	const instances = (await list().catch(handleError)) ?? []

	// Kick off the live CurseForge catalog fetch now so it runs in parallel
	// with the Modrinth queries below — a slow catalog must never stall the
	// rest of the page.
	const liveCatalogPromise = get_curseforge_catalog().catch(() => null)

	// 1. Modrinth Featured Packs
	const filters = []
	filters.push(['project_type:modpack'])

	if (featuredProjects.value.length > 0) {
		filters.push(featuredProjects.value.map((p) => `project_id:${p.id}`))
	} else {
		filters.push(['project_id:none'])
	}

	const facetsParam = JSON.stringify(filters)
	const query = `?facets=${facetsParam}&limit=10&index=follows${filter.value ? `&query=${filter.value}` : ''}`

	const response = await get_search_results(query)

	let mrModpacks = []
	if (response?.result?.hits) {
		const latestVersions = await Promise.all(
			response.result.hits.map(async (hit) => {
				try {
					const project = await get_project(hit.project_id)

					if (!project?.versions?.length) {
						return null
					}

					const versions = await get_version_many(project.versions)
					return versions.sort((a, b) => new Date(b.date_published) - new Date(a.date_published))[0]
				} catch (error) {
					handleError(error)
					return null
				}
			}),
		)

		mrModpacks = response.result.hits.map((hit, index) => {
			const instance = instances.find((p) => p.link?.project_id === hit.project_id)

			const isInstalled = !!instance && instance.install_stage === 'installed'
			installed.value[hit.project_id] = isInstalled

			if (isInstalled && latestVersions[index]) {
				const currentVersion = instance.link.version_id
				const latestVersion = latestVersions[index].id
				hasUpdate.value[hit.project_id] = currentVersion !== latestVersion
			}

			return {
				...hit,
				project_id: hit.project_id,
				project_type: hit.project_type,
				slug: hit.slug,
				latestVersionId: latestVersions[index]?.id,
				source: 'modrinth',
			}
		})
	}

	// 2. CurseForge Dev-Curated Catalog Packs. The catalog is fetched live
	// from the repo so packs can be added/removed without an app update;
	// fall back to the bundled snapshot when the remote fetch fails (e.g.
	// offline, or the file hasn't been pushed yet).
	const liveCurseforgePacks: CurseForgeCatalogPack[] =
		(await liveCatalogPromise) ?? (curseforgePacks as CurseForgeCatalogPack[])
	const cfModpacks = await Promise.all(
		liveCurseforgePacks.map(async (pack) => {
			const packId = `cf-${pack.projectId}`
			const instance = instances.find(
				(p) =>
					p.link?.project_id === String(pack.projectId) ||
					p.name === pack.name,
			)
			// The instance appears in the list the moment it is created on disk —
			// long before downloads, blocked-mod detection, and the
			// Minecraft/loader install finish. Gate "installed" on the exact
			// same `install_stage` signal the sidebar spinner uses, so the
			// button only shows "Play" once the install is genuinely complete
			// (an install in flight is shown via live installProgress events,
			// not by instance existence — see isInstallInProgress).
			const isInstalled = !!instance && instance.install_stage === 'installed'
			installed.value[packId] = isInstalled

			if (isInstalled && instance?.id) {
				const hasUpd = await check_curseforge_pack_update(instance.id).catch(() => false)
				hasUpdate.value[packId] = hasUpd
			} else {
				hasUpdate.value[packId] = false
			}

			return {
				project_id: packId,
				projectId: pack.projectId,
				title: pack.name,
				author: 'CurseForge',
				description: pack.description,
				icon_url: pack.iconUrl,
				gameVersion: pack.gameVersion,
				loader: pack.loader,
				source: 'curseforge',
			}
		}),
	)

	featuredModpacks.value = [...cfModpacks, ...mrModpacks]

	if (!selectedModpackId.value && featuredModpacks.value.length > 0) {
		selectedModpackId.value = featuredModpacks.value[0].project_id
	}
}

watch(selectedModpackId, (newValue) => {
	if (newValue) {
		localStorage.setItem('lastSelectedModpack', newValue)
	}
})

const selectedModpackHasUpdate = computed(() => {
	return selectedModpack.value ? hasUpdate.value[selectedModpack.value.project_id] : false
})

// True while a pack install is genuinely in flight — either an install()
// call initiated on this page, or live cf_install_progress events still
// flowing. Live progress presence keeps the button in its in-progress state
// across page re-mounts mid-install; clearing it on completion (or failure)
// guarantees the button can never get stuck permanently disabled.
const isInstallInProgress = (projectId: string): boolean =>
	!!installing.value[projectId] || !!installProgress.value[projectId]

const install = async (projectId: string) => {
	const pack = featuredModpacks.value.find((m) => m.project_id === projectId)
	installing.value[projectId] = true
	installProgress.value[projectId] = { current: 0, total: 1, bytesDownloaded: 0, totalBytes: 0, message: 'Starting...' }
	// Open the install progress modal
	if (pack) {
		openInstallModal({
			title: pack.title,
			icon_url: pack.icon_url,
			projectId: pack.project_id,
		})
	}
	try {
		if (pack?.source === 'curseforge') {
			const result = await install_curseforge_catalog_pack(pack.projectId, pack.gameVersion)
			installing.value[projectId] = false
			// Close the install modal on completion
			closeInstallModal()
			installProgress.value[projectId] = undefined
			installed.value[projectId] = true
			hasUpdate.value[projectId] = false
			await getInstances()

			if (result.blockedMods.length > 0) {
				showBlockedModsDialog(pack.title, result.instanceId, result.blockedMods)
			}
		} else {
			await installVersion(projectId, null, null, 'HomePage', (version) => {
				installing.value[projectId] = false
				if (version) {
					installed.value[projectId] = true
					hasUpdate.value[projectId] = false
				}
			})
			// Close the install modal on completion
			closeInstallModal()
			// Clear any stale progress for the Modrinth path
			installProgress.value[projectId] = undefined
		}
	} catch (error) {
		closeInstallModal()
		installing.value[projectId] = false
		installProgress.value[projectId] = undefined
		handleError(error)
	}
}

const updateModpack = async (projectId: string) => {
	const pack = featuredModpacks.value.find((m) => m.project_id === projectId)
	if (!updating.value[projectId]) {
		updating.value[projectId] = true
		installProgress.value[projectId] = { current: 0, total: 1, bytesDownloaded: 0, totalBytes: 0, message: 'Starting...' }
		try {
			if (pack?.source === 'curseforge') {
				const instances = (await list().catch(handleError)) ?? []
				const instance = instances.find(
					(i) =>
						i.link?.project_id === String(pack.projectId) ||
						i.name === pack.title,
				)
				if (instance?.id) {
					const result = await install_curseforge_catalog_pack(
						pack.projectId,
						pack.gameVersion,
						instance.id,
					)
					hasUpdate.value[projectId] = false
					// The stage may have temporarily left "installed" (the backend
					// re-runs the Minecraft/loader install during an update), so
					// restore it explicitly rather than waiting for the next
					// instance event.
					installed.value[projectId] = true
					await getInstances()

					if (result.blockedMods.length > 0) {
						showBlockedModsDialog(pack.title, result.instanceId, result.blockedMods)
					}
				}
			} else {
				const instance = (await list()).find((p) => p.link?.project_id === projectId)
				const project = await get_project(projectId)

				if (project?.versions?.length) {
					const versions = await get_version_many(project.versions)
					const latestVersion = versions.sort(
						(a, b) => new Date(b.date_published) - new Date(a.date_published),
					)[0]

					if (instance?.id && latestVersion?.id && instance?.link?.version_id) {
						await update_managed_modrinth_version(instance.id, latestVersion.id)
						hasUpdate.value[projectId] = false
						await getInstances()
					}
				}
			}
		} catch (error) {
			handleError(error)
		} finally {
			updating.value[projectId] = false
			installProgress.value[projectId] = undefined
		}
	}
}

const playing = ref({})

const unlistenProcess = await process_listener((e) => {
	const instances = recentInstances.value
	const instance = instances.find((p) => p.id === e.instance_id)

	if (instance) {
		const pack = featuredModpacks.value.find((m) =>
			m.source === 'curseforge'
				? instance.link?.project_id === String(m.projectId) ||
				  instance.name === m.title
				: instance.link?.project_id === m.project_id,
		)
		if (pack) {
			playing.value[pack.project_id] = e.event !== 'finished'
		}
	}
})

const play = async (projectId: string) => {
	const pack = featuredModpacks.value.find((m) => m.project_id === projectId)
	const instances = (await list().catch(handleError)) ?? []
	const instance = instances.find((p) =>
		pack?.source === 'curseforge'
			? p.link?.project_id === String(pack.projectId) ||
			  p.name === pack.title
			: p.link?.project_id === projectId,
	)
	if (instance) {
		try {
			playing.value[projectId] = true
			await run(instance.id).catch(handleError)
			trackEvent('InstanceStart', {
				loader: instance.loader,
				game_version: instance.game_version,
				source: 'HomePage',
			})
		} catch (error) {
			playing.value[projectId] = false
			handleError(error)
		}
	}
}

const getFeaturedMods = async () => {
	const response = await get_search_results('?facets=[["project_type:mod"]]&limit=10&index=follows')

	if (response) {
		featuredMods.value = response.result.hits
	} else {
		featuredModpacks.value = []
	}
}

const isLoading = ref(true)
Promise.all([
	getInstances(),
	getFeaturedModpacks(),
	getFeaturedMods()
]).finally(() => {
	isLoading.value = false
})

const unlistenInstance = await instance_listener(async (e: { instance_id: string; event: string }) => {
	if (
			e?.event === 'added' ||
			e?.event === 'created' ||
			e?.event === 'edited' ||
			e?.event === 'removed' ||
			e?.event === 'synced'
	) {
		await getInstances()
		await Promise.all([getFeaturedModpacks(), getFeaturedMods()])
	}
})

// Rolling sample history per project for throughput computation. Progress
// events arrive per-file completion (with up to 12 concurrent downloads), so
// consecutive events can be only milliseconds apart — the old
// last-two-events delta produced nonsense "speed" values. Instead keep the
// most recent ~2s of (bytes, time) samples and average the rate across the
// whole window, which converges to the true aggregate download throughput.
const SPEED_WINDOW_MS = 2000
const speedSamples = ref<Record<string, Array<{ bytes: number; time: number }>>>({})

const recordSpeedSample = (key: string, bytes: number) => {
	const now = Date.now()
	const samples = speedSamples.value[key] ?? []
	samples.push({ bytes, time: now })
	// Prune samples older than the window, always keeping the newest sample
	// as the window's leading edge.
	const cutoff = now - SPEED_WINDOW_MS
	while (samples.length > 1 && samples[0].time < cutoff) {
		samples.shift()
	}
	speedSamples.value[key] = samples
}

// Install progress modal state
const installModalVisible = ref(false)
const installModalPack = ref<{
	title: string
	icon_url: string | null
	projectId: string
} | null>(null)

const openInstallModal = (pack: { title: string; icon_url: string | null; projectId: string }) => {
	installModalPack.value = pack
	installModalVisible.value = true
}
const closeInstallModal = () => {
	installModalVisible.value = false
	installModalPack.value = null
}

// Computed from the currently-visible install modal's pack
const installModalProgress = computed(() => {
	const pack = installModalPack.value
	if (!pack) return null
	return installProgress.value[pack.projectId] ?? null
})

const installModalProgressPercent = computed(() => {
	const progress = installModalProgress.value
	if (!progress || progress.total <= 0) return 0
	if (progress.totalBytes > 0) {
		return Math.min(100, (progress.bytesDownloaded / progress.totalBytes) * 100)
	}
	return Math.min(100, (progress.current / progress.total) * 100)
})

const installModalStatusText = computed(() => {
	const progress = installModalProgress.value
	if (!progress) return ''
	const msg = progress.message ?? ''
	const parts: string[] = []
	if (progress.totalBytes > 0 && progress.bytesDownloaded > 0) {
		parts.push(`${formatBytes(progress.bytesDownloaded)} / ${formatBytes(progress.totalBytes)}`)
	}
	const speed = downloadSpeed(installModalPack.value?.projectId ?? '')
	if (speed) {
		parts.push(speed)
	}
	const suffix = parts.length > 0 ? ` — ${parts.join(' · ')}` : ''
	return msg + suffix
})

// Subtitle for the install modal, mapped from the live phase payload so it
// tracks the actual install stage instead of always claiming "Downloading mods".
const installModalPhaseText = computed(() => {
	switch (installModalProgress.value?.phase) {
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

// Payload of the backend `cf_install_progress` event.
interface CfInstallProgressPayload {
	projectId: number
	phase: string
	current: number
	total: number
	bytesDownloaded: number
	totalBytes: number
	message: string | null
}

const unlistenCfProgress = await cf_install_progress_listener((payload: CfInstallProgressPayload) => {
	const key = `cf-${payload.projectId}`
	installProgress.value[key] = {
		phase: payload.phase,
		current: payload.current,
		total: payload.total,
		bytesDownloaded: payload.bytesDownloaded ?? 0,
		totalBytes: payload.totalBytes ?? 0,
		message: payload.message ?? undefined,
	}
	// Record a byte sample for throughput computation. Zero-byte samples
	// (e.g. the initial fetching_pack event) are skipped so the first speed
	// reading isn't diluted by setup overhead.
	if (payload.bytesDownloaded > 0) {
		recordSpeedSample(key, payload.bytesDownloaded)
	}
})

const isDropdownOpen = ref(false)
const dropdownRef = ref(null)

onMounted(() => {
	document.addEventListener('click', handleClickOutside)
})

onUnmounted(() => {
	document.removeEventListener('click', handleClickOutside)
	unlistenProcess()
	unlistenInstance()
	unlistenCfProgress()
})

const handleClickOutside = (event) => {
	if (dropdownRef.value && !dropdownRef.value.contains(event.target)) {
		isDropdownOpen.value = false
	}
}

const selectModpack = (projectId) => {
	selectedModpackId.value = projectId

	setTimeout(() => {
		isDropdownOpen.value = false
	}, 150)
}

// Opens every blocked mod's CurseForge page in the default browser, with a
// small delay between each to avoid the OS/browser flagging it as popup spam.
const openAllLinks = async () => {
	const dialog = blockedModsDialog.value
	if (!dialog || openingAll.value || unresolvedScanMods.value.length === 0) return
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
	const dialog = blockedModsDialog.value
	if (!dialog || scanningDownloads.value || dialog.mods.length === 0) return
	// Only scan mods that haven't been moved yet
	const unresolved = dialog.mods.filter((mod) => {
		const s = scanStatuses.value[mod.fileId]
		return !s || s.status === 'pending' || s.status === 'not_found'
	})
	if (unresolved.length === 0) {
		scanMessage.value = 'All mods have already been moved.'
		return
	}
	scanningDownloads.value = true
	scanMessage.value = null
	try {
		const result = await scan_downloads_for_blocked_mods(dialog.instanceId, unresolved)
		// Don't resurrect the dialog if the user closed it mid-scan.
		if (blockedModsDialog.value !== dialog) return
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

// Opens the blocked-mods dialog for a fresh install/update result, resetting
// per-item scan statuses to "pending".
const showBlockedModsDialog = (
	packTitle: string,
	instanceId: string,
	mods: CurseForgeBlockedMod[],
) => {
	scanMessage.value = null
	const statuses: Record<number, ScanStatusEntry> = {}
	for (const mod of mods) {
		statuses[mod.fileId] = { status: 'pending', destination: null }
	}
	scanStatuses.value = statuses
	blockedModsDialog.value = { packTitle, instanceId, mods }
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

// Formats bytes to a human-readable string (e.g. "1.2 GB", "142 MB", "24.5 KB").
const formatBytes = (bytes: number): string => {
	if (bytes <= 0) return '0 B'
	const units = ['B', 'KB', 'MB', 'GB', 'TB']
	const i = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1)
	const val = bytes / Math.pow(1024, i)
	return `${val < 10 ? val.toFixed(1) : val.toFixed(0)} ${units[i]}`
}

// Computes the current download speed as a human-readable string (e.g.
// "24.5 MB/s") by averaging the bytes downloaded across the last ~2s of
// samples — this reflects the true aggregate throughput even though events
// arrive per-file from 12 concurrent downloads. Only meaningful while files
// are actively downloading.
const downloadSpeed = (projectId: string): string => {
	const progress = installProgress.value[projectId]
	if (!progress || progress.phase !== 'downloading_mods') return ''
	const samples = speedSamples.value[projectId]
	if (!samples || samples.length < 2) return ''
	const first = samples[0]
	const last = samples[samples.length - 1]
	const elapsed = (last.time - first.time) / 1000
	if (elapsed <= 0) return ''
	const bytes = last.bytes - first.bytes
	if (bytes <= 0) return ''
	const bps = bytes / elapsed
	return `${formatBytes(Math.round(bps))}/s`
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

const handleModpackSelection = (projectId) => {
	const selectedElement = document.querySelector(`[data-modpack-id="${projectId}"]`)
	if (selectedElement) {
		selectedElement.style.transform = 'scale(0.95)'
		setTimeout(() => {
			selectedElement.style.transform = 'scale(1)'
		}, 100)
	}

	selectModpack(projectId)
}


</script>

<template>
	<div class="p-6 flex flex-col h-full">
		<!-- Modpack selector section -->
		<div class="flex flex-col flex-grow">
			<div class="pr-[--floating-sidebar-inset]">
				<h1
					v-if="recentInstances.length > 0"
					class="m-0 inline-block rounded-full bg-black/40 px-4 py-1 text-2xl font-extrabold text-white backdrop-blur-md"
				>
					Welcome back!
				</h1>
				<h1
					v-else
					class="m-0 inline-block rounded-full bg-black/40 px-4 py-1 text-2xl font-extrabold text-white backdrop-blur-md"
				>
					Welcome to Ayin Launcher!
				</h1>
				<RecentWorldsList :recent-instances="recentInstances" />
			</div>

			<div class="mt-auto flex justify-between items-center pr-[--floating-sidebar-inset]">
				<template v-if="isLoading">
					<div class="w-[435px] h-[64px] bg-black/10 rounded-lg animate-pulse"></div>
					<div class="w-[300px] h-[64px] bg-black/10 rounded-lg animate-pulse"></div>
				</template>
				<template v-else>
					<div class="flex items-center gap-4">
						<div ref="dropdownRef" class="relative">
						<button
							class="w-[435px] p-4 bg-raised rounded-lg flex items-center justify-between border border-[--brand-gradient-border] transition-all duration-200 ease-out hover:bg-[--color-button-bg] hover:shadow-lg active:scale-[0.98] relative overflow-hidden"
							@click="isDropdownOpen = !isDropdownOpen"
						>
							<div v-if="selectedModpack" class="flex items-center gap-4">
								<img
									v-if="selectedModpack.icon_url"
									:src="selectedModpack.icon_url"
									class="w-8 h-8 rounded transition-transform duration-200 hover:scale-110"
									:alt="selectedModpack.title"
								/>
								<span class="truncate max-w-[320px] transition-colors duration-200">{{
									selectedModpack.title
								}}</span>
							</div>
							<span v-else class="transition-colors duration-200">Select a modpack</span>
							<svg
								class="w-5 h-5 transition-all duration-200"
								:class="{ 'rotate-180': isDropdownOpen }"
								viewBox="0 0 24 24"
								fill="none"
								stroke="currentColor"
							>
								<polyline points="6 9 12 15 18 9"></polyline>
							</svg>
						</button>

						<!-- Dropdown content -->
						<Transition
							enter-active-class="transition-all duration-200 ease-out"
							enter-from-class="opacity-0 transform scale-95 translate-y-2"
							enter-to-class="opacity-100 transform scale-100 translate-y-0"
							leave-active-class="transition-all duration-150 ease-in"
							leave-from-class="opacity-100 transform scale-100 translate-y-0"
							leave-to-class="opacity-0 transform scale-95 translate-y-2"
						>
							<div
								v-if="isDropdownOpen"
								class="absolute z-50 w-full bottom-full mb-2 bg-raised rounded-lg border border-[--brand-gradient-border] shadow-lg max-h-[60vh] overflow-y-auto"
							>
								<div class="p-2">
									<TransitionGroup name="modpack-item" tag="div" appear>
										<div
											v-for="modpack in featuredModpacks"
											:key="modpack.project_id"
											:data-modpack-id="modpack.project_id"
											class="p-4 rounded-lg cursor-pointer hover:bg-[--color-button-bg] mb-2 last:mb-0 transition-all duration-200 ease-out hover:scale-[1.02] hover:shadow-md"
											:class="{
												'bg-[--color-button-bg]': selectedModpackId === modpack.project_id,
											}"
											@click="handleModpackSelection(modpack.project_id)"
										>
											<div class="flex items-center gap-4">
												<img
													v-if="modpack.icon_url"
													:src="modpack.icon_url"
													class="w-12 h-12 rounded transition-transform duration-200"
													:alt="modpack.title"
												/>
												<div class="flex-grow overflow-hidden">
													<h3 class="text-lg font-bold m-0 truncate transition-colors duration-200">
														{{ modpack.title }}
													</h3>
													<p
														class="text-sm text-gray-500 m-0 truncate transition-colors duration-200"
													>
														{{ modpack.author }}
													</p>
												</div>
											</div>
										</div>

									</TransitionGroup>
								</div>
							</div>
						</Transition>
					</div>
				</div>



					<!-- Action buttons -->
					<Transition
						enter-active-class="transition-all duration-300 ease-out"
						enter-from-class="opacity-0 transform scale-95 translate-x-10"
						enter-to-class="opacity-100 transform scale-100 translate-x-0"
						leave-active-class="transition-all duration-200 ease-in"
						leave-from-class="opacity-100 transform scale-100 translate-x-0"
						leave-to-class="opacity-0 transform scale-95 translate-x-10"
					>
						<div v-if="selectedModpack" class="flex-shrink-0 !w-[300px]">
							<template v-if="installed[selectedModpack.project_id] || updating[selectedModpack.project_id]">
								<div class="tactile-button tactile-button--blue">
									<ButtonStyled
										size="2xlarge"
										type="transparent"
										class="!h-[64px] !w-[300px] !min-w-[300px]"
									>
									<button
										:disabled="playing[selectedModpack.project_id] || updating[selectedModpack.project_id]"
										class="flex flex-row items-center justify-center gap-3 !h-full !w-full text-xl font-bold text-white disabled:opacity-50 disabled:cursor-not-allowed"
											@click="
												selectedModpackHasUpdate
													? updateModpack(selectedModpack.project_id)
													: play(selectedModpack.project_id)
											"
										>
											<div class="flex items-center justify-center w-8 h-8">
												<template v-if="updating[selectedModpack.project_id]">
													<span class="loader"></span>
												</template>
												<template v-else-if="selectedModpackHasUpdate">
													<DownloadIcon class="w-8 h-8" />
												</template>
												<template v-else>
													<PlayIcon class="w-8 h-8" />
												</template>
											</div>
											<div class="text-center min-h-[1.5rem] flex items-center justify-center">
												<template v-if="updating[selectedModpack.project_id]"> Updating... </template>
												<template v-else-if="selectedModpackHasUpdate"> Update </template>
												<template v-else>
													{{ playing[selectedModpack.project_id] ? 'Playing...' : 'Play' }}
												</template>
											</div>
										</button>
									</ButtonStyled>
								</div>
							</template>
							<template v-else>
								<div class="tactile-button min-h-[64px] flex flex-col justify-center">
									<ButtonStyled
										size="2xlarge"
										type="transparent"
										class="!h-[64px] !w-[300px] !min-w-[300px]"
									>
										<button
v-tooltip="
													installed[selectedModpack.project_id]
														? 'This project is already installed'
														: null
											"
										:disabled="isInstallInProgress(selectedModpack.project_id)"
										class="flex flex-row items-center justify-center gap-3 !h-full !w-full text-xl font-bold text-white disabled:opacity-50 disabled:cursor-not-allowed"
										@click="install(selectedModpack.project_id)"
									>
										<div class="flex items-center justify-center w-8 h-8">
											<DownloadIcon
												v-if="!isInstallInProgress(selectedModpack.project_id)"
												class="w-8 h-8"
											/>
											<SpinnerIcon v-else class="animate-spin w-5 h-5 shrink-0" />
										</div>
										<div class="text-center min-h-[1.5rem] flex items-center justify-center">
											{{ isInstallInProgress(selectedModpack.project_id) ? 'Installing...' : 'Install' }}
										</div>
										</button>
									</ButtonStyled>
								</div>
							</template>
						</div>
					</Transition>
				</template>
			</div>
		</div>

		<div v-if="!isLoading && featuredModpacks.length === 0" class="col-span-full py-8 text-center">
			<p>No modpacks found.</p>
		</div>
	</div>

	<!-- Install progress modal -->
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
				v-if="installModalVisible"
				class="fixed inset-0 z-[100] flex items-center justify-center bg-black/60 backdrop-blur-sm"
			>
				<div class="w-[480px] rounded-xl bg-raised p-6 border border-[--brand-gradient-border] shadow-2xl">
					<div class="flex items-center gap-4 mb-4">
						<img
							v-if="installModalPack?.icon_url"
							:src="installModalPack.icon_url"
							class="w-12 h-12 rounded shrink-0"
							:alt="installModalPack.title"
						/>
						<div class="min-w-0">
							<h2 class="text-lg font-extrabold m-0 truncate">
								{{ installModalPack?.title ?? 'Installing' }}
							</h2>
							<p class="text-sm text-gray-400 m-0 mt-0.5">
								{{ installModalPhaseText }}
							</p>
						</div>
						<SpinnerIcon class="animate-spin w-6 h-6 shrink-0 ml-auto text-[--color-brand]" />
					</div>

					<!-- Progress bar -->
					<div class="h-2 w-full overflow-hidden rounded-full bg-[--color-button-bg]">
						<div
							class="h-full rounded-full transition-all duration-200 ease-out"
							:style="{
								width: installModalProgressPercent + '%',
								background: 'linear-gradient(to right, var(--color-brand), var(--color-accent-light))',
							}"
						/>
					</div>

					<!-- Status text -->
					<div class="mt-3 text-center">
						<p class="m-0 text-sm font-semibold text-white/90 truncate" :title="installModalStatusText">
							{{ installModalStatusText }}
						</p>
						<p class="m-0 mt-1 text-xs text-gray-400">
							{{ installModalProgress ? `${installModalProgress.current} / ${installModalProgress.total} files` : '' }}
						</p>
					</div>
				</div>
			</div>
		</Transition>
	</Teleport>

	<!-- Blocked mods dialog -->
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
				v-if="blockedModsDialog"
				class="fixed inset-0 z-[100] flex items-center justify-center bg-black/60 backdrop-blur-sm"
				@click.self="blockedModsDialog = null"
			>
				<div class="w-[560px] max-h-[80vh] overflow-y-auto rounded-xl bg-raised p-6 border border-[--brand-gradient-border] shadow-2xl">
					<div class="flex items-start justify-between gap-4 mb-4">
						<div>
							<h2 class="text-xl font-extrabold m-0">Manual downloads required</h2>
							<p class="text-sm text-gray-400 m-0 mt-1">
								{{ blockedModsDialog.mods.length }} mod(s) in {{ blockedModsDialog.packTitle }} disallow
								automated downloads and were skipped. Download them manually, then scan your Downloads
								folder to move them into the instance.
							</p>
						</div>
						<button
							class="shrink-0 rounded-lg p-2 text-gray-400 transition-colors duration-150 hover:bg-[--color-button-bg] hover:text-white"
							@click="blockedModsDialog = null"
						>
							<svg class="w-5 h-5" viewBox="0 0 24 24" fill="none" stroke="currentColor">
								<path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M6 18L18 6M6 6l12 12" />
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
							v-for="mod in blockedModsDialog.mods"
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
						@click="blockedModsDialog = null"
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

/* Add smooth transition for dropdown arrow */
svg {
	transition: transform 0.2s ease;
}

/* Add truncation styles */
.truncate {
	white-space: nowrap;
	overflow: hidden;
	text-overflow: ellipsis;
}

.modpack-item-enter-active,
.modpack-item-leave-active {
	transition: all 0.3s ease;
}

.modpack-item-enter-from {
	opacity: 0;
	transform: translateX(-20px) scale(0.95);
}

.modpack-item-leave-to {
	opacity: 0;
	transform: translateX(20px) scale(0.95);
}

.modpack-item-move {
	transition: transform 0.3s ease;
}

button {
	transition: all 0.2s ease;
}

button:active {
	transform: scale(0.98);
}

/* Prevent the tactile-button wrapper from showing its press animation
   when the inner button is disabled — the disabled state should be visually
   inert. The :has() selector targets the wrapper that contains a disabled
   button, suppressing its active transform. */
.tactile-button:has(button:disabled) {
	filter: grayscale(0.4) !important;
	cursor: not-allowed;
}

.tactile-button:has(button:disabled):hover {
	filter: grayscale(0.4) !important;
}

.tactile-button:has(button:disabled):active {
	transform: none !important;
	box-shadow:
		0 5px 0 var(--tactile-deep),
		0 14px 22px -8px rgba(var(--tactile-sh), 0.5),
		inset 0 1px 0 rgba(255, 255, 255, 0.35) !important;
}

.modpack-item-enter-active .p-4:hover {
	transform: translateY(-2px);
	box-shadow: 0 4px 12px rgba(0, 0, 0, 0.15);
}

img:hover {
	transform: scale(1.05);
}

.bg-\[--color-button-bg\] {
	animation: selectedPulse 0.3s ease-out;
}

@keyframes selectedPulse {
	0% {
		transform: scale(1);
		box-shadow: 0 0 0 0 rgba(var(--color-brand), 0.4);
	}
	50% {
		transform: scale(1.02);
		box-shadow: 0 0 0 8px rgba(var(--color-brand), 0.2);
	}
	100% {
		transform: scale(1);
		box-shadow: 0 0 0 0 rgba(var(--color-brand), 0);
	}
}

.loader {
	position: relative;
	width: 28px;
	height: 28px;
	background: linear-gradient(to right, #fff 20%, #0000 21%);
	background-repeat: repeat-x;
	background-size: 24px 6px;
	background-position: 6px bottom;
	animation: moveX 0.5s linear infinite;
}

.loader::before {
	content: '';
	position: absolute;
	width: 28px;
	height: 28px;
	border-radius: 2px;
	background-color: #fff;
	left: 50%;
	top: 50%;
	transform: translate(-50%, -50%);
	animation: rotate 0.5s linear infinite;
}

@keyframes moveX {
	0%,
	25% {
		background-position: 10px bottom;
	}

	75%,
	100% {
		background-position: -30px bottom;
	}
}

@keyframes rotate {
	0%,
	25% {
		transform: translate(-50%, -50%) rotate(0deg);
	}

	75%,
	100% {
		transform: translate(-55%, -55%) rotate(90deg);
	}
}


</style>
