<script setup lang="ts">
import { computed } from 'vue'

import { parseMinecraftFormatting } from '@/helpers/minecraft-colors'

const props = withDefaults(
	defineProps<{
		text?: string | null
	}>(),
	{ text: '' },
)

const segments = computed(() => parseMinecraftFormatting(props.text ?? ''))
</script>

<template>
	<template v-for="(segment, index) in segments" :key="index">
		<span
			:class="{
				'mc-bold': segment.bold,
				'mc-italic': segment.italic,
				'mc-underline': segment.underline,
				'mc-strikethrough': segment.strikethrough,
				'mc-obfuscated': segment.obfuscated,
			}"
			:style="{ color: segment.color ?? undefined }"
			>{{ segment.text }}</span
		>
	</template>
</template>

<style scoped lang="scss">
.mc-bold {
	font-weight: 700;
}

.mc-italic {
	font-style: italic;
}

.mc-underline {
	text-decoration: underline;
}

.mc-strikethrough {
	text-decoration: line-through;
}

/* Obfuscated (§k) text: the client shuffles characters rapidly. CSS can't
   reshuffle glyphs, so we approximate with a flicker animation. */
.mc-obfuscated {
	animation: mc-obfuscate 0.12s steps(1) infinite;
}

@keyframes mc-obfuscate {
	0% {
		opacity: 1;
	}
	50% {
		opacity: 0.4;
	}
	100% {
		opacity: 1;
	}
}
</style>
