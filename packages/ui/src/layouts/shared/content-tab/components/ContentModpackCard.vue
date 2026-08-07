<script setup lang="ts">
import {
	BoxesIcon,
	CircleAlertIcon,
	ClockIcon,
	DownloadIcon,
	DropdownIcon,
	HeartIcon,
	MoreVerticalIcon,
	Settings2Icon,
	SpinnerIcon,
	XIcon,
} from '@modrinth/assets'
import { Tooltip } from 'floating-vue'
import { computed, getCurrentInstance, onMounted, onUnmounted, ref } from 'vue'
import type { RouteLocationRaw } from 'vue-router'

import AutoLink from '#ui/components/base/AutoLink.vue'
import Avatar from '#ui/components/base/Avatar.vue'
import BulletDivider from '#ui/components/base/BulletDivider.vue'
import ButtonStyled from '#ui/components/base/ButtonStyled.vue'
import FloatingPanel from '#ui/components/base/FloatingPanel.vue'
import OverflowMenu, {
	type Option as OverflowMenuOption,
} from '#ui/components/base/OverflowMenu.vue'
import TagTagItem from '#ui/components/base/TagTagItem.vue'
import TeleportOverflowMenu from '#ui/components/base/TeleportOverflowMenu.vue'
import { useRelativeTime } from '#ui/composables/how-ago'
import { defineMessages, useVIntl } from '#ui/composables/i18n'
import { commonMessages } from '#ui/utils/common-messages'

import type {
	ContentModpackCardCategory,
	ContentModpackCardProject,
	ContentModpackCardVersion,
	ContentModpackVersionOption,
	ContentOwner,
} from '../types'

const { formatMessage } = useVIntl()

const messages = defineMessages({
	contentHintTitle: {
		id: 'content.modpack-card.content-hint-title',
		defaultMessage: 'Modpack content moved',
	},
	contentHintDescription: {
		id: 'content.modpack-card.content-hint-description',
		defaultMessage: "Your modpack's content can now be found here!",
	},
	dismissHint: {
		id: 'content.modpack-card.dismiss-hint',
		defaultMessage: "Don't show again",
	},
	versionDropdownLabel: {
		id: 'content.modpack-card.version-dropdown',
		defaultMessage: 'Modpack version',
	},
	loadingVersions: {
		id: 'content.modpack-card.loading-versions',
		defaultMessage: 'Loading versions...',
	},
	noVersions: {
		id: 'content.modpack-card.no-versions',
		defaultMessage: 'No other versions available',
	},
	currentlyInstalled: {
		id: 'content.modpack-card.currently-installed',
		defaultMessage: 'Currently Installed',
	},
	latest: {
		id: 'content.modpack-card.latest',
		defaultMessage: 'Latest',
	},
	release: {
		id: 'content.modpack-card.release',
		defaultMessage: 'Release',
	},
	beta: {
		id: 'content.modpack-card.beta',
		defaultMessage: 'Beta',
	},
	alpha: {
		id: 'content.modpack-card.alpha',
		defaultMessage: 'Alpha',
	},
})

interface Props {
	project: ContentModpackCardProject
	projectLink?: string | RouteLocationRaw
	version?: ContentModpackCardVersion
	versionLink?: string | RouteLocationRaw
	owner?: ContentOwner
	categories?: ContentModpackCardCategory[]
	disabled?: boolean
	overflowOptions?: OverflowMenuOption[]
	hasUpdate?: boolean
	disabledText?: string
	showContentHint?: boolean
	/** CurseForge catalog-pack version list. When non-null, an inline version dropdown is rendered. */
	versionOptions?: ContentModpackVersionOption[] | null
	versionLoading?: boolean
	versionError?: string | null
	versionBusy?: boolean
	installedVersionId?: number | null
	latestVersionId?: number | null
}

const props = withDefaults(defineProps<Props>(), {
	projectLink: undefined,
	version: undefined,
	versionLink: undefined,
	owner: undefined,
	categories: undefined,
	disabled: false,
	overflowOptions: undefined,
	hasUpdate: false,
	disabledText: undefined,
	showContentHint: false,
	versionOptions: null,
	versionLoading: false,
	versionError: null,
	versionBusy: false,
	installedVersionId: null,
	latestVersionId: null,
})

const emit = defineEmits<{
	update: []
	content: []
	settings: []
	'dismiss-content-hint': []
	'change-version': [fileId: number]
}>()

const instance = getCurrentInstance()
const hasUpdateListener = computed(() => typeof instance?.vnode.props?.onUpdate === 'function')
const hasContentListener = computed(() => typeof instance?.vnode.props?.onContent === 'function')
const hasSettingsListener = computed(() => typeof instance?.vnode.props?.onSettings === 'function')

const formatTimeAgo = useRelativeTime()

const formatCompact = (n: number | undefined) => {
	if (n === undefined) return ''
	return new Intl.NumberFormat('en', { notation: 'compact', maximumFractionDigits: 2 }).format(n)
}

const collapsedOptions = computed(() => {
	const options: {
		id: string
		action: () => void
		color?: 'standard' | 'red' | 'brand' | 'orange' | 'green' | 'blue' | 'purple'
	}[] = []
	if (hasContentListener.value) {
		options.push({
			id: 'content',
			action: () => emit('content'),
		})
	}
	if (hasSettingsListener.value) {
		options.push({
			id: 'settings',
			action: () => emit('settings'),
		})
	}
	return options
})

// Rendered whenever the dropdown is "active": during the initial fetch
// (options still null), on error (options null), or once loaded. Non-CF packs
// pass loading=false/error=null/options=null, so they stay hidden.
const showVersionDropdown = computed(
	() => props.versionLoading || props.versionError !== null || props.versionOptions !== null,
)

const currentVersionLabel = computed(() => {
	const installed = props.versionOptions?.find((o) => o.fileId === props.installedVersionId)
	return (
		installed?.fileName ??
		props.project.filename ??
		formatMessage(commonMessages.selectVersionPlaceholder)
	)
})

function isInstalledVersion(fileId: number) {
	return props.installedVersionId != null && props.installedVersionId === fileId
}

function isLatestVersion(fileId: number) {
	return props.latestVersionId != null && props.latestVersionId === fileId
}

function releaseTypeLabel(releaseType: number) {
	if (releaseType === 1) return formatMessage(messages.release)
	if (releaseType === 2) return formatMessage(messages.beta)
	if (releaseType === 3) return formatMessage(messages.alpha)
	return ''
}

function formatVersionDate(date: string) {
	return new Date(date).toLocaleDateString('en-US', {
		year: 'numeric',
		month: 'short',
		day: 'numeric',
	})
}

function handleVersionSelect(fileId: number) {
	if (props.versionBusy || isInstalledVersion(fileId)) return
	emit('change-version', fileId)
}

const containerRef = ref<HTMLElement | null>(null)
const isExpanded = ref(true)
let observer: ResizeObserver | null = null
onMounted(() => {
	observer = new ResizeObserver((entries) => {
		for (const entry of entries) {
			isExpanded.value = entry.contentRect.width >= 700
		}
	})
	if (containerRef.value) observer.observe(containerRef.value)
})
onUnmounted(() => {
	observer?.disconnect()
	observer = null
})
</script>

<template>
	<div
		ref="containerRef"
		class="@container flex flex-col gap-4 rounded-[20px] bg-bg-raised p-6 shadow-md"
		:class="{ 'opacity-50': disabled }"
	>
		<div class="flex flex-wrap items-start justify-between gap-4">
			<div class="flex min-w-0 flex-1 items-center gap-4">
				<AutoLink :to="projectLink" class="shrink-0">
					<Avatar :src="project.icon_url" :alt="project.title" size="5rem" no-shadow raised />
				</AutoLink>
				<div class="flex min-w-0 flex-col gap-1.5">
					<div class="flex min-w-0 flex-col">
						<AutoLink
							:to="projectLink"
							class="truncate text-xl font-semibold text-contrast"
							:class="projectLink ? 'hover:underline' : ''"
						>
							{{ project.title }}
						</AutoLink>
						<span
							v-if="project.filename && (owner || version)"
							class="truncate text-secondary mb-2"
						>
							{{ project.filename }}
						</span>
					</div>
					<div
						v-if="project.filename && !(owner || version)"
						class="flex min-h-8 min-w-0 items-center text-secondary"
					>
						<span class="truncate">{{ project.filename }}</span>
					</div>
					<div
						v-if="owner || version"
						class="flex flex-nowrap items-center gap-2 overflow-hidden text-secondary"
					>
						<AutoLink
							v-if="owner"
							:to="owner.link"
							class="flex shrink-0 items-center gap-1.5 hover:underline"
						>
							<Avatar
								:src="owner.avatar_url"
								:alt="owner.name"
								size="2rem"
								:circle="owner.type === 'user'"
								no-shadow
							/>
							<span class="font-medium whitespace-nowrap">{{ owner.name }}</span>
						</AutoLink>
						<template v-if="version">
							<BulletDivider v-if="owner" />
							<AutoLink
								:to="versionLink"
								class="shrink-0 font-medium text-secondary !decoration-secondary whitespace-nowrap"
								:class="versionLink ? 'hover:underline' : ''"
							>
								{{ version.version_number }}
							</AutoLink>
						</template>
						<template v-if="version?.date_published">
							<BulletDivider />
							<div class="flex shrink-0 items-center gap-2 whitespace-nowrap">
								<ClockIcon class="size-5" />
								<span>{{ formatTimeAgo(new Date(version.date_published)) }}</span>
							</div>
						</template>
					</div>
				</div>
			</div>

			<div class="flex shrink-0 items-center gap-2">
				<template v-if="disabled">
					<div class="flex items-center gap-2 text-secondary">
						<SpinnerIcon class="animate-spin" />
						<span class="font-semibold">{{
							disabledText ?? formatMessage(commonMessages.updatingLabel)
						}}</span>
					</div>
				</template>
				<template v-else>
					<!-- Expanded actions visible at >= 700px -->
					<div class="hidden @[700px]:flex items-center gap-2">
						<ButtonStyled
							v-if="hasUpdateListener && hasUpdate"
							type="transparent"
							color="green"
							color-fill="text"
						>
							<button class="flex items-center gap-2" @click="emit('update')">
								<DownloadIcon class="!text-green" />
								<span class="font-semibold">{{ formatMessage(commonMessages.updateButton) }}</span>
							</button>
						</ButtonStyled>

						<Tooltip
							v-if="hasContentListener"
							theme="dismissable-prompt"
							class="inline-flex"
							:triggers="[]"
							:shown="showContentHint && isExpanded"
							:auto-hide="false"
							placement="bottom-end"
						>
							<ButtonStyled>
								<button
									class="!shadow-none"
									@click="
										() => {
											emit('content')
											emit('dismiss-content-hint')
										}
									"
								>
									<BoxesIcon />
									{{ formatMessage(commonMessages.contentLabel) }}
								</button>
							</ButtonStyled>
							<template #popper>
								<div class="grid grid-cols-[min-content] gap-1">
									<div class="flex min-w-48 items-center justify-between gap-8">
										<h3 class="m-0 whitespace-nowrap text-base font-bold text-contrast">
											{{ formatMessage(messages.contentHintTitle) }}
										</h3>
										<ButtonStyled size="small" circular>
											<button
												v-tooltip="formatMessage(messages.dismissHint)"
												@click="emit('dismiss-content-hint')"
											>
												<XIcon aria-hidden="true" />
											</button>
										</ButtonStyled>
									</div>
									<p class="m-0 text-wrap text-sm font-medium leading-tight text-secondary">
										{{ formatMessage(messages.contentHintDescription) }}
									</p>
								</div>
							</template>
						</Tooltip>

						<FloatingPanel
							v-if="showVersionDropdown"
							v-tooltip="formatMessage(messages.versionDropdownLabel)"
							placement="bottom-end"
							:disabled="props.versionBusy"
							button-class="!h-9 gap-1.5 max-w-[240px]"
							panel-class="!p-1.5 w-80"
							:auto-focus="false"
						>
							<DropdownIcon class="size-4 shrink-0" />
							<span class="truncate text-sm font-semibold">{{ currentVersionLabel }}</span>
							<template #panel>
								<div class="flex max-h-80 flex-col gap-0.5 overflow-y-auto">
									<div
										v-if="props.versionLoading"
										class="flex items-center justify-center gap-2 py-8 text-secondary"
									>
										<SpinnerIcon class="size-5 animate-spin" />
										<span class="text-sm">{{ formatMessage(messages.loadingVersions) }}</span>
									</div>
									<div
										v-else-if="props.versionError"
										class="flex items-center gap-2 px-2 py-6 text-sm text-red"
									>
										<CircleAlertIcon class="size-5 shrink-0" />
										<span class="min-w-0 break-words">{{ props.versionError }}</span>
									</div>
									<div
										v-else-if="!props.versionOptions?.length"
										class="px-2 py-6 text-sm text-secondary"
									>
										{{ formatMessage(messages.noVersions) }}
									</div>
									<template v-else>
										<button
											v-for="option in props.versionOptions"
											:key="option.fileId"
											class="flex w-full flex-col gap-0.5 rounded-lg px-2.5 py-2 text-left transition-colors duration-150 hover:bg-button-bg"
											:class="isInstalledVersion(option.fileId) ? 'bg-button-bg' : ''"
											:disabled="props.versionBusy"
											@click="handleVersionSelect(option.fileId)"
										>
											<span class="flex items-center justify-between gap-2">
												<span class="min-w-0 truncate text-sm font-semibold text-contrast">
													{{ option.fileName }}
												</span>
												<span class="flex shrink-0 items-center gap-1">
													<span
														v-if="option.releaseType"
														class="rounded-full bg-surface-5 px-1.5 py-0.5 text-[10px] font-medium text-secondary"
													>
														{{ releaseTypeLabel(option.releaseType) }}
													</span>
													<span
														v-if="
															isLatestVersion(option.fileId) && !isInstalledVersion(option.fileId)
														"
														class="rounded-full bg-surface-5 px-1.5 py-0.5 text-[10px] font-semibold text-contrast"
													>
														{{ formatMessage(messages.latest) }}
													</span>
													<span
														v-if="isInstalledVersion(option.fileId)"
														class="rounded-full bg-brand px-1.5 py-0.5 text-[10px] font-semibold text-white"
													>
														{{ formatMessage(messages.currentlyInstalled) }}
													</span>
												</span>
											</span>
											<span
												v-if="option.fileDate || option.gameVersions?.length"
												class="flex items-center gap-1.5 text-xs text-secondary"
											>
												<span v-if="option.fileDate">{{ formatVersionDate(option.fileDate) }}</span>
												<template v-if="option.gameVersions?.length">
													<span v-if="option.fileDate">·</span>
													<span class="truncate">{{ option.gameVersions.join(', ') }}</span>
												</template>
											</span>
										</button>
									</template>
								</div>
							</template>
						</FloatingPanel>

						<ButtonStyled v-if="hasSettingsListener" type="outlined" circular>
							<button
								@click="
									() => {
										emit('settings')
										emit('dismiss-content-hint')
									}
								"
							>
								<Settings2Icon />
							</button>
						</ButtonStyled>
					</div>

					<!-- Collapsed actions visible at < 700px -->
					<div v-if="hasUpdate && hasUpdateListener" class="flex @[700px]:hidden">
						<ButtonStyled circular type="transparent" color="green" color-fill="text">
							<button
								v-tooltip="formatMessage(commonMessages.updateButton)"
								@click="emit('update')"
							>
								<DownloadIcon class="size-5" />
							</button>
						</ButtonStyled>
					</div>
					<Tooltip
						v-if="collapsedOptions.length"
						theme="dismissable-prompt"
						class="inline-flex"
						:triggers="[]"
						:shown="showContentHint && !isExpanded"
						:auto-hide="false"
						placement="bottom-end"
					>
						<ButtonStyled circular type="outlined"
							><TeleportOverflowMenu
								:options="collapsedOptions"
								class="flex @[700px]:hidden"
								@open="emit('dismiss-content-hint')"
							>
								<MoreVerticalIcon class="size-5" />
								<template #content>
									<BoxesIcon class="size-5" />
									{{ formatMessage(commonMessages.contentLabel) }}
								</template>
								<template #settings>
									<Settings2Icon class="size-5" />
									{{ formatMessage(commonMessages.settingsLabel) }}
								</template>
							</TeleportOverflowMenu></ButtonStyled
						>
						<template #popper>
							<div class="grid grid-cols-[min-content] gap-1">
								<div class="flex min-w-48 items-center justify-between gap-8">
									<h3 class="m-0 whitespace-nowrap text-base font-bold text-contrast">
										{{ formatMessage(messages.contentHintTitle) }}
									</h3>
									<ButtonStyled size="small" circular>
										<button
											v-tooltip="formatMessage(messages.dismissHint)"
											@click="emit('dismiss-content-hint')"
										>
											<XIcon aria-hidden="true" />
										</button>
									</ButtonStyled>
								</div>
								<p class="m-0 text-wrap text-sm font-medium leading-tight text-secondary">
									{{ formatMessage(messages.contentHintDescription) }}
								</p>
							</div>
						</template>
					</Tooltip>

					<ButtonStyled
						v-if="overflowOptions?.length"
						circular
						type="transparent"
						class="hidden @[700px]:flex"
					>
						<OverflowMenu :options="overflowOptions">
							<MoreVerticalIcon class="size-5" />
						</OverflowMenu>
					</ButtonStyled>
				</template>
			</div>
		</div>

		<span v-if="project.description" class="text-secondary">
			{{ project.description }}
		</span>

		<div
			v-if="project.downloads != null || project.followers != null || categories?.length"
			class="flex flex-wrap items-center gap-3"
		>
			<div v-if="project.downloads != null" class="flex items-center gap-2 text-secondary">
				<DownloadIcon class="size-5" />
				<span class="font-medium">{{ formatCompact(project.downloads) }}</span>
			</div>

			<div v-if="project.followers != null" class="flex items-center gap-2 text-secondary">
				<HeartIcon class="size-5" />
				<span class="font-medium">{{ formatCompact(project.followers) }}</span>
			</div>

			<div v-if="categories?.length" class="flex flex-wrap items-center gap-1">
				<TagTagItem
					v-for="cat in categories"
					:key="cat.name"
					:tag="cat.name"
					:action="cat.action"
					hide-non-loader-icon
				/>
			</div>
		</div>
	</div>
</template>
