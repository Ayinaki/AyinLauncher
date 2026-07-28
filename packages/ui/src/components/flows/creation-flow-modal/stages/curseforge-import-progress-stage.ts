import { markRaw } from 'vue'
import type { StageConfigInput } from '../../../base'
import type { CreationFlowContextValue } from '../creation-flow-context'
import CurseforgeImportProgressStage from '../components/CurseforgeImportProgressStage.vue'

export const stageConfig: StageConfigInput<CreationFlowContextValue> = {
	id: 'curseforge-import-progress',
	title: () => 'Importing Modpack',
	stageContent: markRaw(CurseforgeImportProgressStage),
	leftButtonConfig: null,
	rightButtonConfig: null,
	maxWidth: '520px',
	noblur: true,
}
