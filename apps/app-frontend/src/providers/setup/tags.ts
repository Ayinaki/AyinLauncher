import type { AbstractWebNotificationManager } from '@modrinth/ui'
import { provideTags } from '@modrinth/ui'
import { ref } from 'vue'

import { ensureStateInitialized } from '@/helpers/state'
import { get_game_versions, get_loaders } from '@/helpers/tags'

export function setupTagsProvider(notificationManager: AbstractWebNotificationManager) {
	const { handleError } = notificationManager

	const gameVersions = ref([])
	const loaders = ref([])

	const loadTags = async () => {
		try {
			await ensureStateInitialized()
			gameVersions.value = await get_game_versions()
			loaders.value = await get_loaders()
		} catch (err) {
			handleError(err)
		}
	}

	void loadTags()

	provideTags({ gameVersions, loaders })
}
