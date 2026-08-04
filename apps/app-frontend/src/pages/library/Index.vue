<script setup lang="ts">
import { injectNotificationManager } from '@modrinth/ui'
import { onUnmounted, shallowRef, watch } from 'vue'
import { useRoute } from 'vue-router'

import { NewInstanceImage } from '@/assets/icons'
import { instance_listener } from '@/helpers/events.js'
import { list } from '@/helpers/instance'
import { useBreadcrumbs } from '@/store/breadcrumbs.js'

const { handleError } = injectNotificationManager()
const route = useRoute()
const breadcrumbs = useBreadcrumbs()

breadcrumbs.setRootContext({ name: 'Library', link: route.path })

const instances = shallowRef(await list().catch(handleError))

// Re-fetch instances whenever the Library route becomes active (not just on
// mount). Vue Router reuses the component instance when navigating back to
// /library because the route key is the same, so the top-level await above
// only runs once. This watcher ensures the list is fresh on every visit.
watch(
	() => route.path,
	(path) => {
		if (path.startsWith('/library')) {
			list().catch(handleError).then((result) => {
				if (result) instances.value = result
			})
		}
	},
)

const unlistenInstance = await instance_listener(async (e: any) => {
	if (e?.event === 'added' || e?.event === 'created' || e?.event === 'removed' || e?.event === 'synced') {
		instances.value = await list().catch(handleError)
	}
})
onUnmounted(() => {
	unlistenInstance()
})
</script>

<template>
	<div class="library-container">
		<div class="p-6 pt-6 flex-1 min-h-0">
			<template v-if="instances && instances.length > 0">
				<Suspense>
					<RouterView v-if="route.path.startsWith('/library')" :instances="instances" />
				</Suspense>
			</template>
			<div v-else class="no-instance">
				<div class="icon">
					<NewInstanceImage />
				</div>
				<h3>No instances found</h3>
				<p class="no-instance-description">Install a modpack from the Home page to get started</p>
			</div>
		</div>
	</div>
</template>

<style lang="scss" scoped>
.library-container {
	display: flex;
	flex-direction: column;
	height: 100%;
	/* No background here: like the other pages, the Library is transparent so
	   the app's blurred background image (rendered behind the viewport in
	   App.vue) shows through. An opaque background would cover it. */
	overflow: hidden;
}

.no-instance {
	display: flex;
	flex-direction: column;
	align-items: center;
	justify-content: center;
	height: 60vh;
	gap: var(--gap-lg);
	text-align: center;

	p,
	h3 {
		margin: 0;
	}

	.no-instance-description {
		color: var(--color-secondary);
		font-size: 1.125rem;
		max-width: 400px;
	}

	.icon {
		svg {
			width: 12rem;
			height: 12rem;
			opacity: 0.7;
		}
	}
}

.blur-background {
	backdrop-filter: blur(5px);
	height: 82vh;
}
</style>
