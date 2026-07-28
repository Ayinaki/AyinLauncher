import type { StageConfigInput } from '../../../base'
import type { CreationFlowContextValue } from '../creation-flow-context'
import { stageConfig as customSetupStageConfig } from './custom-setup-stage'
import { stageConfig as finalConfigStageConfig } from './final-config-stage'
import { stageConfig as importInstanceStageConfig } from './import-instance-stage'
import { stageConfig as modpackStageConfig } from './modpack-stage'
import { stageConfig as setupTypeStageConfig } from './setup-type-stage'
import { stageConfig as curseforgeImportProgressStageConfig } from './curseforge-import-progress-stage'
import { stageConfig as curseforgeBlockedModsStageConfig } from './curseforge-blocked-mods-stage'

export const stageConfigs: StageConfigInput<CreationFlowContextValue>[] = [
	setupTypeStageConfig,
	modpackStageConfig,
	importInstanceStageConfig,
	curseforgeImportProgressStageConfig,
	curseforgeBlockedModsStageConfig,
	customSetupStageConfig,
	finalConfigStageConfig,
]
