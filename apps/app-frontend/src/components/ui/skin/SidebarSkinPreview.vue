<script setup lang="ts">
import { DualSkinPreview, injectNotificationManager } from '@modrinth/ui'
import { computed, ref } from 'vue'

import { get_available_skins, get_normalized_skin_texture, determineModelType, SkinModel } from '@/helpers/skins'
import { useTheming } from '@/store/state'

const PARTNER_SKIN = `https://mc-heads.net/skin/Ayinaki`

const { handleError } = injectNotificationManager()
const themeStore = useTheming()

const skinTexture = ref('')
const skinModel = ref<SkinModel>('CLASSIC')
const partnerModel = ref<SkinModel>('CLASSIC')
const debug = computed(() => themeStore.getFeatureFlag('dual_skin_debug'))

const isLoading = ref(true)

// Guards against overlapping refreshes (e.g. rapid account switches) resolving
// out of order: only the latest call is allowed to write skinTexture.
let refreshToken = 0

async function refresh() {
	isLoading.value = true
	const token = ++refreshToken
	const commit = (value: string, model: SkinModel, partner: SkinModel) => {
		if (token === refreshToken) {
			skinTexture.value = value
			skinModel.value = model
			partnerModel.value = partner
			isLoading.value = false
		}
	}

	const skins = await get_available_skins().catch(handleError)
	const equipped = (skins ?? []).find((skin) => skin.is_equipped) ?? null

	if (!equipped?.texture) {
		commit('', 'CLASSIC', 'CLASSIC')
		return
	}

	const partnerVariantPromise = determineModelType(PARTNER_SKIN).catch(() => 'CLASSIC' as SkinModel)

	try {
		const tex = await get_normalized_skin_texture(equipped)
		const partnerVariant = await partnerVariantPromise
		commit(tex, equipped.variant, partnerVariant)
	} catch (error) {
		if (equipped.texture.startsWith('data:image/')) {
			const partnerVariant = await partnerVariantPromise
			commit(equipped.texture, equipped.variant, partnerVariant)
		} else {
			handleError(error as Error)
			commit('', 'CLASSIC', 'CLASSIC')
		}
	}
}

defineExpose({ refresh })

refresh()
</script>

<template>
	<!-- Height lives here so the sidebar slot collapses when there's no skin. -->
	<div v-if="isLoading" class="h-[22rem] bg-black/10 rounded-lg animate-pulse"></div>
	<div v-else-if="skinTexture" class="h-[22rem]">
		<DualSkinPreview
			:left-texture-src="skinTexture"
			:left-model="skinModel"
			:right-texture-src="PARTNER_SKIN"
			:right-model="partnerModel"
			:debug="debug"
		/>
	</div>
</template>
