<script setup lang="ts">
import { Button } from '@modrinth/ui'
import { invoke } from '@tauri-apps/api/core'
import { check, type Update } from '@tauri-apps/plugin-updater'
import { ref } from 'vue'

import AyinAppLogo from '@/assets/ayin_app.svg?component'

import ModalWrapper from './ModalWrapper.vue'

const modal = ref<InstanceType<typeof ModalWrapper> | null>(null)
const updateObj = ref<Update | null>(null)
const version = ref<string>('')

type UpdateStage = 'idle' | 'available' | 'downloading' | 'installed'
const stage = ref<UpdateStage>('idle')
const downloadPercent = ref<number>(0)
const downloadError = ref<string | null>(null)

async function checkForMandatoryUpdate(): Promise<boolean> {
	try {
		const update = await check()
		if (update && update.available) {
			updateObj.value = update
			version.value = update.version
			stage.value = 'available'
			modal.value?.show()
			return true
		}
	} catch (error) {
		console.warn('Silent update check failed:', error)
	}
	return false
}

async function startUpdate() {
	if (!updateObj.value) return
	stage.value = 'downloading'
	downloadPercent.value = 0
	downloadError.value = null

	try {
		let downloaded = 0
		let contentLength = 0

		await updateObj.value.downloadAndInstall((event) => {
			switch (event.event) {
				case 'Started':
					contentLength = event.data.contentLength ?? 0
					break
				case 'Progress':
					downloaded += event.data.chunkLength
					if (contentLength > 0) {
						downloadPercent.value = Math.min(
							99,
							Math.round((downloaded / contentLength) * 100),
						)
					}
					break
				case 'Finished':
					downloadPercent.value = 100
					break
			}
		})

		stage.value = 'installed'
	} catch (err: any) {
		console.error('Failed to download and install update:', err)
		downloadError.value = err?.message || 'An error occurred during update installation.'
		stage.value = 'available'
	}
}

async function restartApp() {
	try {
		await invoke('restart_app')
	} catch (err) {
		console.error('Failed to restart app:', err)
	}
}

defineExpose({
	checkForMandatoryUpdate,
})
</script>

<template>
	<ModalWrapper ref="modal" :closable="false" class="mandatory-update-modal-wrapper">
		<template #title>
			<div class="modal-title">
				<h1 class="title-text">
					<template v-if="stage === 'installed'">Update Ready</template>
					<template v-else-if="stage === 'downloading'">Updating Ayin Launcher</template>
					<template v-else>Mandatory Update Required</template>
				</h1>
			</div>
		</template>

		<div class="mandatory-update-container">
			<div class="logo-wrapper">
				<AyinAppLogo class="w-16 h-16 logo-icon" />
			</div>

			<!-- Stage 1: Update Available -->
			<div v-if="stage === 'available'" class="stage-content">
				<p class="update-description">
					A new version of Ayin Launcher (<strong class="version-tag">v{{ version }}</strong
					>) is required to continue.
				</p>
				<p v-if="downloadError" class="error-text">
					{{ downloadError }}
				</p>
				<div class="action-wrapper">
					<Button color="brand" class="update-btn" @click="startUpdate"> Update Now </Button>
				</div>
			</div>

			<!-- Stage 2: Downloading & Installing -->
			<div v-else-if="stage === 'downloading'" class="stage-content">
				<p class="update-description">
					Downloading and installing update <strong>v{{ version }}</strong
					>...
				</p>

				<div class="progress-bar-container">
					<div class="progress-bar-fill" :style="{ width: downloadPercent + '%' }"></div>
				</div>
				<p class="progress-text">{{ downloadPercent }}% complete</p>

				<p class="subtext">Please do not close Ayin Launcher.</p>
			</div>

			<!-- Stage 3: Installed / Ready to Relaunch -->
			<div v-else-if="stage === 'installed'" class="stage-content">
				<p class="update-description">
					Update <strong>v{{ version }}</strong> has been installed successfully!
				</p>
				<p class="subtext">Restart Ayin Launcher to apply the update.</p>

				<div class="action-wrapper">
					<Button color="brand" class="update-btn" @click="restartApp">
						Restart to Apply Update
					</Button>
				</div>
			</div>
		</div>
	</ModalWrapper>
</template>

<style scoped>
:deep(.mandatory-update-modal-wrapper) {
	--modal-width: 480px !important;
	width: 480px !important;
}

.modal-title {
	text-align: center;
	padding: 0.5rem 0;
}

.title-text {
	font-size: 1.5rem;
	font-weight: 700;
	margin: 0;
	color: var(--color-contrast, #ffffff);
}

.mandatory-update-container {
	display: flex;
	flex-direction: column;
	align-items: center;
	padding: 1.5rem 1rem 1rem 1rem;
	text-align: center;
}

.logo-wrapper {
	margin-bottom: 1.25rem;
	display: flex;
	justify-content: center;
}

.logo-icon {
	width: 4rem;
	height: 4rem;
}

.stage-content {
	width: 100%;
	display: flex;
	flex-direction: column;
	align-items: center;
	gap: 1rem;
}

.update-description {
	font-size: 1rem;
	color: var(--color-base, #e0e0e0);
	margin: 0;
	line-height: 1.5;
}

.version-tag {
	color: var(--color-brand, #3b82f6);
}

.subtext {
	font-size: 0.875rem;
	color: var(--color-subtle, #9ca3af);
	margin: 0;
}

.error-text {
	font-size: 0.875rem;
	color: #ef4444;
	margin: 0;
}

.action-wrapper {
	width: 100%;
	display: flex;
	justify-content: center;
	margin-top: 0.5rem;
}

.update-btn {
	width: 100%;
	justify-content: center;
	padding: 0.75rem 1.5rem;
	font-size: 1rem;
	font-weight: 600;
}

.progress-bar-container {
	width: 100%;
	height: 10px;
	background: rgba(255, 255, 255, 0.1);
	border-radius: 5px;
	overflow: hidden;
	margin-top: 0.5rem;
}

.progress-bar-fill {
	height: 100%;
	background: var(--color-brand, #3b82f6);
	transition: width 0.2s ease-out;
}

.progress-text {
	font-size: 0.875rem;
	font-weight: 600;
	color: var(--color-contrast, #ffffff);
	margin: 0;
}
</style>
