<script setup>
import { SpinnerIcon } from '@modrinth/assets'
import { Avatar, injectNotificationManager } from '@modrinth/ui'
import { convertFileSrc } from '@tauri-apps/api/core'
import dayjs from 'dayjs'
import { onUnmounted, ref } from 'vue'

import NavButton from '@/components/ui/NavButton.vue'
import { instance_listener } from '@/helpers/events.js'
import { list } from '@/helpers/instance'
import { stripMinecraftCodes } from '@/helpers/minecraft-colors'

const { handleError } = injectNotificationManager()

// How many of the most recently created/played instances the sidebar rail
// shows. The rail scrolls (with hidden scrollbars) past this point, so the
// number is a preference, not a hard ceiling.
const MAX_QUICK_INSTANCES = 8

const recentInstances = ref([])
const getInstances = async () => {
	const instances = await list().catch(handleError)

	recentInstances.value = instances
		.sort((a, b) => {
			const dateACreated = dayjs(a.created)
			const dateAPlayed = a.last_played ? dayjs(a.last_played) : dayjs(0)

			const dateBCreated = dayjs(b.created)
			const dateBPlayed = b.last_played ? dayjs(b.last_played) : dayjs(0)

			const dateA = dateACreated.isAfter(dateAPlayed) ? dateACreated : dateAPlayed
			const dateB = dateBCreated.isAfter(dateBPlayed) ? dateBCreated : dateBPlayed

			if (dateA.isSame(dateB)) {
				return a.name.localeCompare(b.name)
			}

			return dateB - dateA
		})
		.slice(0, MAX_QUICK_INSTANCES)
}

await getInstances()

const unlistenInstance = await instance_listener(async (event) => {
	if (event.event !== 'synced') {
		await getInstances()
	}
})

onUnmounted(() => {
	unlistenInstance()
})
</script>

<template>
	<!-- The rail grows to fit all capped instances. The parent nav column is
	     the scroll container (hidden scrollbars), so a long rail scrolls the
	     whole column instead of clipping mid-icon or pushing the bottom nav
	     buttons off-screen. -->
	<div class="quick-instances flex flex-col gap-[0.5rem]">
		<div
			v-for="instance in recentInstances"
			:key="instance.id"
			v-tooltip.right="stripMinecraftCodes(instance.name)"
		>
			<NavButton :to="`/instance/${encodeURIComponent(instance.id)}`" class="relative">
				<Avatar
					:src="instance.icon_path ? convertFileSrc(instance.icon_path) : null"
					size="28px"
					:tint-by="instance.id"
					:class="`transition-all ${instance.install_stage !== 'installed' ? `brightness-[0.25] scale-[0.85]` : `group-hover:brightness-75`}`"
				/>
				<div
					v-if="instance.install_stage !== 'installed'"
					class="absolute inset-0 flex items-center justify-center z-10 pointer-events-none"
				>
					<SpinnerIcon class="animate-spin w-4 h-4" />
				</div>
			</NavButton>
		</div>
	</div>
</template>
