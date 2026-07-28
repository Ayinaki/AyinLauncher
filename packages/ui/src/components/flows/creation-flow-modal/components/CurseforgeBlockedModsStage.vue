<template>
	<div class="flex flex-col gap-4">
		<div class="text-sm text-secondary">
			{{ formatMessage(messages.blockedModsDescription) }}
		</div>

		<div class="flex flex-col gap-2 max-h-[260px] overflow-y-auto bg-surface-1 rounded-md p-2">
			<div
				v-for="(mod, index) in ctx.curseforgeBlockedMods.value"
				:key="mod.projectId || mod.expectedFileName || index"
				class="flex items-center justify-between gap-2 p-2 bg-surface-2 rounded-md text-sm border border-solid"
				:class="mod.found ? 'border-green-500/30 bg-green-500/5' : 'border-surface-5'"
			>
				<div class="flex flex-col min-w-0">
					<div class="flex items-center gap-2">
						<span class="truncate text-contrast font-mono text-xs font-semibold">{{ mod.expectedFileName || `File #${mod.projectId || index}` }}</span>
						<span v-if="mod.classId === 12 || (mod.expectedFileName && mod.expectedFileName.endsWith('.zip') && mod.classId !== 6 && mod.classId !== 6552)" class="text-[10px] px-1.5 py-0.5 rounded bg-surface-3 text-secondary font-sans font-medium shrink-0">Resource Pack</span>
						<span v-else-if="mod.classId === 6552" class="text-[10px] px-1.5 py-0.5 rounded bg-surface-3 text-secondary font-sans font-medium shrink-0">Shader Pack</span>
					</div>
					<span v-if="mod.found" class="text-[11px] text-green-400 font-medium">✓ Found and copied</span>
					<span v-else class="text-[11px] text-amber-400">Missing — Download from CurseForge</span>
				</div>
				<ButtonStyled v-if="mod.pageUrl && !mod.found" type="outlined" size="small">
					<button class="text-xs px-2 py-1 flex items-center gap-1" @click="handleOpenSingleUrl(mod.pageUrl)">
						Open Link
					</button>
				</ButtonStyled>
			</div>
		</div>

		<div class="flex gap-2">
			<ButtonStyled v-if="hasUrls" type="outlined" class="flex-1">
				<button class="w-full" @click="handleOpenAllLinks">
					{{ formatMessage(messages.openAllLinksButton) }}
				</button>
			</ButtonStyled>

			<ButtonStyled type="outlined" class="flex-1">
				<button class="w-full" :disabled="isScanning" @click="() => handleScanFolder()">
					{{ isScanning ? formatMessage(messages.scanningLabel) : formatMessage(messages.scanFolderButton) }}
				</button>
			</ButtonStyled>
		</div>

		<div class="bg-surface-1 rounded-md p-3 border border-solid border-surface-5 flex items-center justify-between gap-4 mt-2">
			<div class="flex items-center gap-2 min-w-0">
				<div v-if="remainingCount === 0" class="flex items-center gap-2 text-xs text-green-400 font-semibold bg-green-500/10 px-3 py-2 rounded-md border border-solid border-green-500/20">
					<svg class="w-4 h-4 text-green-400 shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
						<path stroke-linecap="round" stroke-linejoin="round" d="M5 13l4 4L19 7" />
					</svg>
					<span>All mods found! Ready to import.</span>
				</div>
				<div v-else class="flex items-center gap-2 text-xs text-amber-400 font-medium bg-amber-500/10 px-3 py-2 rounded-md border border-solid border-amber-500/20">
					<svg class="w-4 h-4 text-amber-400 shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
						<path stroke-linecap="round" stroke-linejoin="round" d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-3L13.732 4c-.77-1.333-2.694-1.333-3.464 0L3.34 16c-.77 1.333.192 3 1.732 3z" />
					</svg>
					<span>{{ remainingCount }} mod{{ remainingCount === 1 ? '' : 's' }} remaining</span>
				</div>
			</div>
			<ButtonStyled color="brand">
				<button class="px-5 py-2 text-sm font-semibold whitespace-nowrap" @click="handleFinish">
					Finish Import
				</button>
			</ButtonStyled>
		</div>
	</div>
</template>

<script setup lang="ts">
import { defineMessages, useVIntl } from '@modrinth/ui'
import { computed, ref } from 'vue'
import { injectCreationFlowContext } from '../creation-flow-context'
import { injectNotificationManager } from '../../../../providers/web-notifications'
import ButtonStyled from '../../../base/ButtonStyled.vue'

const ctx = injectCreationFlowContext()
const { formatMessage } = useVIntl()
const notificationManager = injectNotificationManager()

const isScanning = ref(false)

const remainingCount = computed(() => {
	return ctx.curseforgeBlockedMods.value.filter((m: any) => !m.found).length
})

const hasUrls = computed(() => {
	return ctx.curseforgeBlockedMods.value.some((m: any) => !!m.pageUrl && !m.found)
})

const messages = defineMessages({
	blockedModsDescription: {
		id: 'creation-flow.curseforge.blocked-mods.description',
		defaultMessage: 'Some mods in this modpack disallow third-party automated downloads. Click "Open All Links" to download them manually to your Downloads folder, then click "Scan Downloads Folder".',
	},
	openAllLinksButton: {
		id: 'creation-flow.curseforge.blocked-mods.open-all-button',
		defaultMessage: 'Open All Links',
	},
	scanFolderButton: {
		id: 'creation-flow.curseforge.blocked-mods.scan-button',
		defaultMessage: 'Scan Downloads Folder',
	},
	scanningLabel: {
		id: 'creation-flow.curseforge.blocked-mods.scanning',
		defaultMessage: 'Scanning...',
	},
	allModsFound: {
		id: 'creation-flow.curseforge.blocked-mods.all-found',
		defaultMessage: 'All mods found! Ready to import.',
	},
	modsRemaining: {
		id: 'creation-flow.curseforge.blocked-mods.remaining',
		defaultMessage: '{count} mods remaining.',
	},
	scanSuccess: {
		id: 'creation-flow.curseforge.blocked-mods.scan-success',
		defaultMessage: 'Successfully found {count} mods!',
	},
	scanError: {
		id: 'creation-flow.curseforge.blocked-mods.scan-error',
		defaultMessage: 'Failed to scan folder for mods. {error}',
	},
})

async function handleOpenSingleUrl(url: string) {
	try {
		await ctx.openCfUrls([url])
	} catch {
		window.open(url, '_blank', 'noopener,noreferrer')
	}
}

async function handleOpenAllLinks() {
	const urls = ctx.curseforgeBlockedMods.value
		.filter((m: any) => !m.found && !!m.pageUrl)
		.map((m: any) => m.pageUrl as string)

	if (urls.length === 0) return

	try {
		await ctx.openCfUrls(urls)
	} catch {
		for (const url of urls) {
			window.open(url, '_blank', 'noopener,noreferrer')
		}
	}
}

async function handleScanFolder(customFolder?: string) {
	if (isScanning.value) return
	
	try {
		isScanning.value = true
		const result = await ctx.scanFolderForMods(customFolder || '')
		
		// Update found flags on blocked mods array
		const foundNames = new Set(
			ctx.curseforgeBlockedMods.value.length - result.remaining.length > 0
				? ctx.curseforgeBlockedMods.value
						.map((m: any) => m.expectedFileName)
						.filter((name: string) => !result.remaining.some((r: any) => r.expectedFileName === name))
				: []
		)
		
		ctx.curseforgeBlockedMods.value = ctx.curseforgeBlockedMods.value.map((m: any) => ({
			...m,
			found: m.found || foundNames.has(m.expectedFileName),
		}))
	} catch (error: any) {
		notificationManager.addNotification({
			title: 'Error Scanning Folder',
			description: formatMessage(messages.scanError, { error: error.message || String(error) }),
			type: 'error',
		})
	} finally {
		isScanning.value = false
	}
}

function handleFinish() {
	ctx.finish()
}
</script>
