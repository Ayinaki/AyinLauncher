<script setup lang="ts">
import {
	DownloadIcon,
	EyeIcon,
	FolderOpenIcon,
	GameIcon,
	PlayIcon,
	SpinnerIcon,
	StopCircleIcon,
	TimerIcon,
} from '@modrinth/assets'
import { Avatar, ButtonStyled, injectNotificationManager, useRelativeTime } from '@modrinth/ui'
import { convertFileSrc } from '@tauri-apps/api/core'
import dayjs from 'dayjs'
import { computed, onMounted, onUnmounted, ref } from 'vue'
import { useRouter } from 'vue-router'

import MinecraftText from '@/components/ui/MinecraftText.vue'
import { trackEvent } from '@/helpers/analytics'
import { process_listener } from '@/helpers/events'
import { install_existing_instance, install_pack_to_existing_instance } from '@/helpers/install'
import { kill, run } from '@/helpers/instance'
import { stripMinecraftCodes } from '@/helpers/minecraft-colors'
import { get_by_instance_id } from '@/helpers/process'
import { showInstanceInFolder } from '@/helpers/utils.js'
import { handleSevereError } from '@/store/error.js'

const { handleError } = injectNotificationManager()
const formatRelativeTime = useRelativeTime()

const props = defineProps({
	instance: {
		type: Object,
		default() {
			return {}
		},
	},
	compact: {
		type: Boolean,
		default: false,
	},
	first: {
		type: Boolean,
		default: false,
	},
})

const playing = ref(false)
const loading = ref(false)
const modLoading = computed(
	() =>
		loading.value ||
		currentEvent.value === 'installing' ||
		(currentEvent.value === 'launched' && !playing.value),
)
const installing = computed(() => props.instance.install_stage.includes('installing'))
const installed = computed(() => props.instance.install_stage === 'installed')

const router = useRouter()

const seeInstance = async () => {
	await router.push(`/instance/${encodeURIComponent(props.instance.id)}`)
}

const checkProcess = async () => {
	const runningProcesses = await get_by_instance_id(props.instance.id).catch(handleError)

	playing.value = runningProcesses.length > 0
}

const play = async (e, context) => {
	e?.stopPropagation()
	loading.value = true
	await run(props.instance.id)
		.catch((err) => handleSevereError(err, { instanceId: props.instance.id }))
		.finally(() => {
			trackEvent('InstanceStart', {
				loader: props.instance.loader,
				game_version: props.instance.game_version,
				source: context,
			})
		})
	loading.value = false
}

const stop = async (e, context) => {
	e?.stopPropagation()
	playing.value = false

	await kill(props.instance.id).catch(handleError)

	trackEvent('InstanceStop', {
		loader: props.instance.loader,
		game_version: props.instance.game_version,
		source: context,
	})
}

const repair = async (e) => {
	e?.stopPropagation()

	if (
		props.instance.install_stage !== 'pack_installed' &&
		(props.instance.link?.type === 'modrinth_modpack' ||
			props.instance.link?.type === 'server_project_modpack')
	) {
		await install_pack_to_existing_instance(props.instance.id, {
			type: 'fromVersionId',
			project_id: props.instance.link.project_id ?? props.instance.link.server_project_id ?? '',
			version_id: props.instance.link.version_id ?? props.instance.link.content_version_id ?? '',
			title: stripMinecraftCodes(props.instance.name),
		}).catch(handleError)
	} else {
		await install_existing_instance(props.instance.id, false).catch(handleError)
	}
}

const openFolder = async () => {
	await showInstanceInFolder(props.instance.id)
}

const addContent = async () => {
	await router.push({
		path: `/browse/${props.instance.loader === 'vanilla' ? 'datapack' : 'mod'}`,
		query: { i: props.instance.id },
	})
}

defineExpose({
	play,
	stop,
	seeInstance,
	openFolder,
	addContent,
	instance: props.instance,
})

const currentEvent = ref(null)

const unlisten = await process_listener((e) => {
	if (e.instance_id === props.instance.id) {
		currentEvent.value = e.event
		if (e.event === 'finished') {
			playing.value = false
		}
	}
})

onMounted(() => checkProcess())
onUnmounted(() => unlisten())
const getIconUrl = (path: string | null | undefined): string | undefined => {
	if (!path) return undefined
	if (/^(https?:|data:|blob:|asset:|tauri:)/.test(path)) return path
	const clean = path.startsWith('\\\\?\\') ? path.slice(4) : path
	return convertFileSrc(clean)
}
</script>

<template>
	<template v-if="compact">
		<div
			class="card-shadow grid grid-cols-[auto_1fr_auto] bg-bg-raised rounded-xl p-3 pl-4 gap-2 cursor-pointer hover:brightness-90 transition-all"
			@click="seeInstance"
			@mouseenter="checkProcess"
		>
			<Avatar
				size="48px"
				:src="getIconUrl(instance.icon_path)"
				:tint-by="instance.id"
				alt="Mod card"
			/>
			<div class="h-full flex items-center font-bold text-contrast leading-normal">
				<span class="line-clamp-2"><MinecraftText :text="instance.name" /></span>
			</div>
			<div class="flex items-center">
				<ButtonStyled v-if="playing" color="red" circular @mousehover="checkProcess">
					<button v-tooltip="'Stop'" @click="(e) => stop(e, 'InstanceCard')">
						<StopCircleIcon />
					</button>
				</ButtonStyled>
				<ButtonStyled v-else-if="modLoading" color="standard" circular>
					<button v-tooltip="'Instance is loading...'" disabled>
						<SpinnerIcon class="animate-spin" />
					</button>
				</ButtonStyled>
				<ButtonStyled v-else :color="first ? 'brand' : 'standard'" circular>
					<button
						v-tooltip="'Play'"
						@click="(e) => play(e, 'InstanceCard')"
						@mousehover="checkProcess"
					>
						<!-- Translate for optical centering -->
						<PlayIcon class="translate-x-[1px]" />
					</button>
				</ButtonStyled>
			</div>
			<div class="flex items-center col-span-3 gap-1 text-secondary font-semibold">
				<TimerIcon />
				<span class="text-sm">
					<template v-if="instance.last_played">
						Played {{ formatRelativeTime(dayjs(instance.last_played).toISOString()) }}
					</template>
					<template v-else> Never played </template>
				</span>
			</div>
		</div>
	</template>
	<div
		v-else
		class="instance-card"
		role="button"
		tabindex="0"
		aria-label="Open {{ instance.name }}"
		@click="seeInstance"
		@keydown.enter="seeInstance"
		@keydown.space.prevent="seeInstance"
		@mouseenter="checkProcess"
	>
		<!-- Secondary actions, revealed on hover (top-right) -->
		<div class="quick-actions" @click.stop>
			<button v-tooltip="'View instance'" class="quick-action" @click="seeInstance">
				<EyeIcon />
			</button>
			<button v-tooltip="'Open folder'" class="quick-action" @click.stop="openFolder">
				<FolderOpenIcon />
			</button>
		</div>

		<!-- Live state badge (top-left) -->
		<div v-if="playing" class="playing-badge">
			<span class="playing-dot"></span>
			Playing
		</div>

		<div class="card-header">
			<div class="icon-wrap">
				<Avatar
					size="92px"
					:src="getIconUrl(instance.icon_path)"
					:tint-by="instance.id"
					alt="Instance icon"
					:class="`instance-icon ${modLoading || installing ? 'loading' : ''}`"
				/>
			</div>
		</div>

		<div class="card-body">
			<h3 class="instance-name"><MinecraftText :text="instance.name" /></h3>

			<div class="instance-details">
				<div class="detail-item">
					<GameIcon class="detail-icon" />
					<span class="detail-text capitalize">
						{{ instance.loader }} {{ instance.game_version }}
					</span>
				</div>

				<div class="detail-item">
					<TimerIcon class="detail-icon" />
					<span class="detail-text">
						<template v-if="instance.last_played">
							Played {{ formatRelativeTime(dayjs(instance.last_played).toISOString()) }}
						</template>
						<template v-else> Never played </template>
					</span>
				</div>
			</div>
		</div>

		<div class="card-footer">
			<button
				v-if="playing"
				v-tooltip="'Stop'"
				class="primary-action stop-action"
				@click.stop="(e) => stop(e, 'InstanceCard')"
			>
				<StopCircleIcon />
				<span>Stop</span>
			</button>
			<button v-else-if="modLoading || installing" class="primary-action" disabled>
				<SpinnerIcon class="animate-spin" />
				<span>{{ installing ? 'Installing…' : 'Starting…' }}</span>
			</button>
			<button
				v-else-if="!installed"
				v-tooltip="'Repair installation'"
				class="primary-action repair-action"
				@click.stop="(e) => repair(e)"
			>
				<DownloadIcon />
				<span>Repair</span>
			</button>
			<button
				v-else
				v-tooltip="'Play'"
				class="primary-action"
				@click.stop="(e) => play(e, 'InstanceCard')"
			>
				<PlayIcon class="translate-x-[1px]" />
				<span>Play</span>
			</button>
		</div>
	</div>
</template>

<style lang="scss" scoped>
.instance-card {
	position: relative;
	display: flex;
	flex-direction: column;
	background: color-mix(in srgb, var(--color-bg) 78%, #000 22%);
	border: 1px solid rgba(255, 255, 255, 0.1);
	border-radius: 1rem;
	overflow: hidden;
	cursor: pointer;
	transition:
		transform 0.25s ease,
		box-shadow 0.25s ease,
		border-color 0.25s ease;
	box-shadow: 0 4px 16px rgba(0, 0, 0, 0.25);

	&:hover,
	&:focus-visible {
		transform: translateY(-3px);
		border-color: rgba(255, 255, 255, 0.18);
		box-shadow: 0 12px 32px rgba(0, 0, 0, 0.35);
		outline: none;

		.quick-actions {
			opacity: 1;
			transform: translateY(0);
		}

		.icon-wrap .instance-icon {
			transform: scale(1.04);
		}
	}

	.quick-actions {
		position: absolute;
		top: 0.75rem;
		right: 0.75rem;
		z-index: 2;
		display: flex;
		gap: 0.375rem;
		opacity: 0;
		transform: translateY(-4px);
		transition:
			opacity 0.2s ease,
			transform 0.2s ease;

		.quick-action {
			display: flex;
			align-items: center;
			justify-content: center;
			width: 2rem;
			height: 2rem;
			border-radius: 0.6rem;
			border: 1px solid rgba(255, 255, 255, 0.12);
			background: rgba(0, 0, 0, 0.45);
			color: rgba(255, 255, 255, 0.85);
			cursor: pointer;
			backdrop-filter: blur(4px);
			transition:
				background 0.15s ease,
				color 0.15s ease;

			&:hover {
				background: rgba(255, 255, 255, 0.15);
				color: #fff;
			}

			svg {
				width: 1rem;
				height: 1rem;
			}
		}
	}

	.playing-badge {
		position: absolute;
		top: 0.75rem;
		left: 0.75rem;
		z-index: 2;
		display: flex;
		align-items: center;
		gap: 0.4rem;
		padding: 0.3rem 0.65rem;
		border-radius: 999px;
		border: 1px solid rgba(74, 222, 128, 0.35);
		background: rgba(0, 0, 0, 0.45);
		color: #4ade80;
		font-size: 0.68rem;
		font-weight: 800;
		letter-spacing: 0.05em;
		text-transform: uppercase;
		backdrop-filter: blur(4px);

		.playing-dot {
			width: 0.5rem;
			height: 0.5rem;
			border-radius: 50%;
			background: #4ade80;
			animation: pulse-dot 1.2s ease-in-out infinite;
		}
	}

	.card-header {
		display: flex;
		justify-content: center;
		padding: 1.75rem 1.25rem 0.875rem;

		.icon-wrap {
			padding: 7px;
			border-radius: 1.35rem;
			background: rgba(255, 255, 255, 0.06);
			box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.09);

			.instance-icon {
				width: 84px !important;
				height: 84px !important;
				border-radius: 1.05rem !important;
				transition: transform 0.3s ease;

				&.loading {
					opacity: 0.5;
				}
			}
		}
	}

	.card-body {
		flex: 1;
		display: flex;
		flex-direction: column;
		gap: 0.875rem;
		padding: 0 1.25rem;

		.instance-name {
			margin: 0;
			font-size: 1.2rem;
			font-weight: 800;
			color: var(--color-contrast);
			line-height: 1.25;
			text-align: center;
			word-break: break-word;
		}

		.instance-details {
			display: flex;
			flex-direction: column;
			gap: 0.375rem;
			.detail-item {
				display: flex;
				align-items: center;
				gap: 0.625rem;
				padding: 0.5rem 0.75rem;
				border-radius: 0.625rem;
				border: 1px solid rgba(255, 255, 255, 0.08);
				background: rgba(255, 255, 255, 0.08);

				.detail-icon {
					width: 0.875rem;
					height: 0.875rem;
					color: #83cdfb;
					flex-shrink: 0;
				}

				.detail-text {
					font-size: 0.8125rem;
					color: #c6d2e4;
					font-weight: 700;
					line-height: 1.2;
				}
			}
		}
	}

	.card-footer {
		padding: 1rem 1.25rem 1.25rem;

		.primary-action {
			display: flex;
			align-items: center;
			justify-content: center;
			gap: 0.5rem;
			width: 100%;
			padding: 0.7rem;
			border: none;
			border-radius: 0.75rem;
			background: linear-gradient(135deg, var(--color-brand), var(--color-accent-light));
			box-shadow: 0 4px 14px -4px color-mix(in srgb, var(--color-brand) 50%, transparent);
			color: #fff;
			font-size: 0.9375rem;
			font-weight: 800;
			letter-spacing: 0.02em;
			cursor: pointer;
			transition:
				transform 0.15s ease,
				box-shadow 0.15s ease,
				filter 0.15s ease;

			&:hover:not(:disabled) {
				transform: translateY(-1px);
				filter: brightness(1.08);
				box-shadow: 0 8px 20px -6px color-mix(in srgb, var(--color-brand) 60%, transparent);
			}

			&:active:not(:disabled) {
				transform: translateY(0) scale(0.98);
			}

			&:disabled {
				opacity: 0.75;
				cursor: default;
			}

			&.stop-action {
				background: linear-gradient(135deg, #ef4444, #b91c1c);
				box-shadow: 0 4px 14px -4px color-mix(in srgb, #ef4444 50%, transparent);

				&:hover:not(:disabled) {
					box-shadow: 0 8px 20px -6px color-mix(in srgb, #ef4444 60%, transparent);
				}
			}

			&.repair-action {
				background: rgba(255, 255, 255, 0.1);
				box-shadow: none;

				&:hover:not(:disabled) {
					background: rgba(255, 255, 255, 0.16);
					box-shadow: none;
				}
			}

			svg {
				width: 1.125rem;
				height: 1.125rem;
			}
		}
	}
}

@keyframes pulse-dot {
	0%,
	100% {
		opacity: 1;
		transform: scale(1);
	}
	50% {
		opacity: 0.55;
		transform: scale(0.8);
	}
}

// Responsive adjustments
@media (max-width: 768px) {
	.instance-card {
		.card-header {
			padding: 1.25rem 1rem 0.625rem;

			.icon-wrap .instance-icon {
				width: 64px !important;
				height: 64px !important;
			}
		}

		.card-body .instance-name {
			font-size: 1.125rem;
		}

		.card-body {
			gap: 0.75rem;
			padding: 0 1rem;
		}

		.card-footer {
			padding: 0.75rem 1rem 1rem;
		}
	}
}

@media (max-width: 480px) {
	.instance-card {
		.card-header {
			padding: 1rem 0.875rem 0.5rem;
		}

		.card-body {
			padding: 0 0.875rem;

			.instance-details .detail-item {
				padding: 0.375rem;

				.detail-text {
					font-size: 0.8125rem;
				}
			}
		}
	}
}
</style>
