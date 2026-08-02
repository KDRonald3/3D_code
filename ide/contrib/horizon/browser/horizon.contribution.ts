/*---------------------------------------------------------------------------------------------
 *  Copyright (c) Horizon contributors. All rights reserved.
 *--------------------------------------------------------------------------------------------*/

import { Codicon } from '../../../../base/common/codicons.js';
import { Disposable } from '../../../../base/common/lifecycle.js';
import { localize, localize2 } from '../../../../nls.js';
import { Action2, MenuId, MenuRegistry, registerAction2 } from '../../../../platform/actions/common/actions.js';
import { IInstantiationService, ServicesAccessor } from '../../../../platform/instantiation/common/instantiation.js';
import { SyncDescriptor } from '../../../../platform/instantiation/common/descriptors.js';
import { KeyCode, KeyMod } from '../../../../base/common/keyCodes.js';
import { KeybindingWeight } from '../../../../platform/keybinding/common/keybindingsRegistry.js';
import { Registry } from '../../../../platform/registry/common/platform.js';
import { EditorPaneDescriptor, IEditorPaneRegistry } from '../../../browser/editor.js';
import { IWorkbenchContribution, registerWorkbenchContribution2, WorkbenchPhase } from '../../../common/contributions.js';
import { EditorExtensions, IEditorFactoryRegistry, IEditorSerializer } from '../../../common/editor.js';
import { EditorInput } from '../../../common/editor/editorInput.js';
import { IEditorResolverService, RegisteredEditorPriority } from '../../../services/editor/common/editorResolverService.js';
import { IEditorService } from '../../../services/editor/common/editorService.js';
import {
	HORIZON_CMD_ANALYSE,
	HORIZON_CMD_HIDE,
	HORIZON_CMD_OPEN,
	HORIZON_CMD_SHOW,
	HORIZON_CMD_TOGGLE,
} from '../common/horizon.js';
import { HorizonMapEditorPane } from './horizonEditorPane.js';
import { HorizonMapInput } from './horizonInput.js';

//#region --- EditorPane registration

Registry.as<IEditorPaneRegistry>(EditorExtensions.EditorPane).registerEditorPane(
	EditorPaneDescriptor.create(
		HorizonMapEditorPane,
		HorizonMapEditorPane.ID,
		localize('horizonMapEditor', "Horizon Map")
	),
	[new SyncDescriptor(HorizonMapInput)]
);

//#endregion

//#region --- Editor resolver + serializer

class HorizonMapEditorContribution extends Disposable implements IWorkbenchContribution {

	static readonly ID = 'workbench.contrib.horizonMapEditor';

	constructor(
		@IEditorResolverService editorResolverService: IEditorResolverService,
	) {
		super();

		this._register(editorResolverService.registerEditor(
			`${HorizonMapInput.RESOURCE.scheme}:**/**`,
			{
				id: HorizonMapInput.ID,
				label: localize('promptOpenWith.horizonMap.displayName', "Horizon Map"),
				priority: RegisteredEditorPriority.exclusive
			},
			{
				singlePerResource: true,
				canSupportResource: resource => resource.scheme === HorizonMapInput.RESOURCE.scheme
			},
			{
				createEditorInput: () => ({
					editor: HorizonMapInput.instance,
					options: { pinned: true }
				})
			}
		));
	}
}

registerWorkbenchContribution2(HorizonMapEditorContribution.ID, HorizonMapEditorContribution, WorkbenchPhase.BlockStartup);

class HorizonMapInputSerializer implements IEditorSerializer {

	canSerialize(_editorInput: EditorInput): boolean {
		return true;
	}

	serialize(_editorInput: EditorInput): string {
		return '';
	}

	deserialize(_instantiationService: IInstantiationService): EditorInput {
		return HorizonMapInput.instance;
	}
}

Registry.as<IEditorFactoryRegistry>(EditorExtensions.EditorFactory).registerEditorSerializer(
	HorizonMapInput.ID,
	HorizonMapInputSerializer
);

//#endregion

//#region --- Commands

const horizonCategory = localize2('horizonCategory', 'Horizon');

async function openHorizonMap(accessor: ServicesAccessor): Promise<HorizonMapEditorPane | undefined> {
	const editorService = accessor.get(IEditorService);
	const pane = await editorService.openEditor({
		resource: HorizonMapInput.RESOURCE,
		options: {
			pinned: true,
			revealIfOpened: true,
			preserveFocus: false,
		}
	});
	return pane instanceof HorizonMapEditorPane ? pane : undefined;
}

async function showClassicEditors(accessor: ServicesAccessor): Promise<void> {
	const editorService = accessor.get(IEditorService);
	const activePane = editorService.activeEditorPane;
	const active = editorService.activeEditor;
	if (active instanceof HorizonMapInput && activePane) {
		await editorService.closeEditor({ editor: active, groupId: activePane.group.id });
	}
}

registerAction2(class extends Action2 {
	constructor() {
		super({
			id: HORIZON_CMD_OPEN,
			title: localize2('horizonMapOpen', 'Open Horizon Map'),
			category: horizonCategory,
			f1: true,
			icon: Codicon.map,
			keybinding: {
				weight: KeybindingWeight.WorkbenchContrib,
				primary: KeyMod.CtrlCmd | KeyMod.Shift | KeyCode.KeyM,
			},
		});
	}
	async run(accessor: ServicesAccessor): Promise<void> {
		await openHorizonMap(accessor);
	}
});

registerAction2(class extends Action2 {
	constructor() {
		super({
			id: HORIZON_CMD_SHOW,
			title: localize2('horizonMapShow', 'Show Horizon Map'),
			category: horizonCategory,
			f1: true,
		});
	}
	async run(accessor: ServicesAccessor): Promise<void> {
		await openHorizonMap(accessor);
	}
});

registerAction2(class extends Action2 {
	constructor() {
		super({
			id: HORIZON_CMD_HIDE,
			title: localize2('horizonMapHide', 'Hide Horizon Map'),
			category: horizonCategory,
			f1: true,
		});
	}
	async run(accessor: ServicesAccessor): Promise<void> {
		await showClassicEditors(accessor);
	}
});

registerAction2(class extends Action2 {
	constructor() {
		super({
			id: HORIZON_CMD_TOGGLE,
			title: localize2('horizonMapToggle', 'Toggle Horizon Map'),
			category: horizonCategory,
			f1: true,
			icon: Codicon.map,
			keybinding: {
				weight: KeybindingWeight.WorkbenchContrib,
				primary: KeyMod.CtrlCmd | KeyMod.Alt | KeyCode.KeyM,
			},
		});
	}
	async run(accessor: ServicesAccessor): Promise<void> {
		const editorService = accessor.get(IEditorService);
		if (editorService.activeEditor instanceof HorizonMapInput) {
			await showClassicEditors(accessor);
		} else {
			await openHorizonMap(accessor);
		}
	}
});

registerAction2(class extends Action2 {
	constructor() {
		super({
			id: HORIZON_CMD_ANALYSE,
			title: localize2('horizonMapAnalyse', 'Analyse Workspace (Horizon Map)'),
			category: horizonCategory,
			f1: true,
			icon: Codicon.sync,
		});
	}
	async run(accessor: ServicesAccessor): Promise<void> {
		const pane = await openHorizonMap(accessor);
		await pane?.analyseWorkspace();
	}
});

MenuRegistry.appendMenuItem(MenuId.MenubarViewMenu, {
	group: '4_horizon',
	command: {
		id: HORIZON_CMD_TOGGLE,
		title: localize({ key: 'miToggleHorizonMap', comment: ['&& denotes a mnemonic'] }, "&&Horizon Map")
	},
	order: 1
});

//#endregion
