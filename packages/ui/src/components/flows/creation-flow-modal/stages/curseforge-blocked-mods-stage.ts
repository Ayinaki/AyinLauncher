import { markRaw } from 'vue'
import type { StageConfigInput } from '../../../base'
import type { CreationFlowContextValue } from '../creation-flow-context'
import CurseforgeBlockedModsStage from '../components/CurseforgeBlockedModsStage.vue'

export const stageConfig: StageConfigInput<CreationFlowContextValue> = {
	id: 'curseforge-blocked-mods',
	title: () => 'Blocked Mods Found',
	stageContent: markRaw(CurseforgeBlockedModsStage),
	leftButtonConfig: null,
	rightButtonConfig: null,
	maxWidth: '560px',
}
