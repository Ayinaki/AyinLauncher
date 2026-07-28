<template>
	<div class="flex flex-col items-center justify-center p-8 gap-4 text-center">
		<SpinnerIcon class="w-12 h-12 animate-spin text-primary" />
		<div class="text-lg font-semibold text-contrast">
			{{ formatMessage(messages.importingTitle) }}
		</div>
		<div class="text-sm text-secondary">
			{{ formatMessage(messages.importingDescription) }}
		</div>
	</div>
</template>

<script setup lang="ts">
import { SpinnerIcon } from '@modrinth/assets'
import { defineMessages, useVIntl } from '@modrinth/ui'

const { formatMessage } = useVIntl()

const messages = defineMessages({
	importingTitle: {
		id: 'creation-flow.curseforge.progress.title',
		defaultMessage: 'Importing CurseForge Modpack...',
	},
	importingDescription: {
		id: 'creation-flow.curseforge.progress.description',
		defaultMessage: 'Downloading mods and extracting files. This may take a moment...',
	},
})
</script>
