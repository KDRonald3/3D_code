/*---------------------------------------------------------------------------------------------
 *  Copyright (c) Horizon contributors. All rights reserved.
 *--------------------------------------------------------------------------------------------*/

import { Codicon } from '../../../../base/common/codicons.js';
import { Disposable } from '../../../../base/common/lifecycle.js';
import { localize, localize2 } from '../../../../nls.js';
import { Action2, MenuId, MenuRegistry, registerAction2 } from '../../../../platform/actions/common/actions.js';
import { ContextKeyExpr } from '../../../../platform/contextkey/common/contextkey.js';
import { IInstantiationService, ServicesAccessor } from '../../../../platform/instantiation/common/instantiation.js';
import { InstantiationType, registerSingleton } from '../../../../platform/instantiation/common/extensions.js';
import { SyncDescriptor } from '../../../../platform/instantiation/common/descriptors.js';
import { KeyCode, KeyMod } from '../../../../base/common/keyCodes.js';
import { KeybindingWeight } from '../../../../platform/keybinding/common/keybindingsRegistry.js';
import { INotificationService, Severity } from '../../../../platform/notification/common/notification.js';
import { Registry } from '../../../../platform/registry/common/platform.js';
import { ConfigurationScope, Extensions as ConfigurationExtensions, IConfigurationRegistry } from '../../../../platform/configuration/common/configurationRegistry.js';
import { registerIcon } from '../../../../platform/theme/common/iconRegistry.js';
import { IWorkspaceContextService } from '../../../../platform/workspace/common/workspace.js';
import { EditorPaneDescriptor, IEditorPaneRegistry } from '../../../browser/editor.js';
import { ViewPaneContainer } from '../../../browser/parts/views/viewPaneContainer.js';
import { IWorkbenchContribution, registerWorkbenchContribution2, WorkbenchPhase } from '../../../common/contributions.js';
import { EditorExtensions, IEditorFactoryRegistry, IEditorSerializer } from '../../../common/editor.js';
import { EditorInput } from '../../../common/editor/editorInput.js';
import {
	Extensions as ViewExtensions,
	IViewContainersRegistry,
	IViewDescriptor,
	IViewsRegistry,
	ViewContainerLocation,
} from '../../../common/views.js';
import { IEditorResolverService, RegisteredEditorPriority } from '../../../services/editor/common/editorResolverService.js';
import { IEditorService } from '../../../services/editor/common/editorService.js';
import { IStatusbarEntryAccessor, IStatusbarService, StatusbarAlignment } from '../../../services/statusbar/browser/statusbar.js';
import {
	HORIZON_CMD_ANALYSE,
	HORIZON_CMD_CHOOSE_FOLDER,
	HORIZON_CMD_HIDE,
	HORIZON_CMD_OPEN,
	HORIZON_CMD_SHOW,
	HORIZON_CMD_TOGGLE,
	HORIZON_CMD_OPEN_SELECTED_FUNCTION,
	HORIZON_CMD_TRIGGER_INSPECT_SUGGEST,
	HORIZON_INSPECTION_ACTIVE_CONTEXT,
	HORIZON_MAP_VISIBLE_CONTEXT,
} from '../common/horizon.js';
import { HorizonAnalysisService, IHorizonAnalysisService } from './horizonAnalysis.js';
import { HorizonMapEditorPane, HorizonMapVisibleContext } from './horizonEditorPane.js';
import { HorizonInspectionService, IHorizonInspectionService } from './horizonInspection.js';
import { HorizonMapInput } from './horizonInput.js';
import { HorizonSidebarView } from './horizonSidebar.js';

// Attach-only sidecar (env URL). Desktop overrides via electron-browser contribution.
import './horizonSidecarService.js';

//#region --- Services (browser attach; electron-browser registers spawn sidecar)

registerSingleton(IHorizonAnalysisService, HorizonAnalysisService, InstantiationType.Delayed);
registerSingleton(IHorizonInspectionService, HorizonInspectionService, InstantiationType.Delayed);

//#endregion

//#region --- Configuration

// The desktop build discovers the sidecar through HORIZON_SIDECAR_URL, but `env`
// is hardcoded to `{}` in web (base/common/process.ts), so the browser workbench
// has no way to reach a sidecar without a setting.
Registry.as<IConfigurationRegistry>(ConfigurationExtensions.Configuration).registerConfiguration({
	id: 'horizon',
	order: 100,
	title: localize('horizonConfigurationTitle', "Horizon"),
	type: 'object',
	properties: {
		'horizon.sidecarUrl': {
			type: 'string',
			default: '',
			scope: ConfigurationScope.APPLICATION,
			description: localize(
				'horizonSidecarUrl',
				"Loopback URL of the horizon-server sidecar used for analyse and source preview, e.g. http://127.0.0.1:8787. Takes precedence over the HORIZON_SIDECAR_URL environment variable. Required in the browser, where environment variables are unavailable."
			),
		},
	},
});

//#endregion

//#region --- Icons / activity bar

const horizonViewIcon = registerIcon(
	'horizon-view-icon',
	Codicon.map,
	localize('horizonViewIcon', 'Icon for the Horizon activity bar.')
);

/** Built-in activity-bar id — must NOT use `workbench.view.extension.*` (extension namespace). */
const HORIZON_VIEWLET_ID = 'workbench.view.horizon';

const horizonViewContainer = Registry.as<IViewContainersRegistry>(ViewExtensions.ViewContainersRegistry).registerViewContainer({
	id: HORIZON_VIEWLET_ID,
	title: localize2('horizonViewContainer', 'Horizon'),
	ctorDescriptor: new SyncDescriptor(ViewPaneContainer, [HORIZON_VIEWLET_ID, { mergeViewWithContainerWhenSingleView: true }]),
	icon: horizonViewIcon,
	order: 5,
	hideIfEmpty: false,
	// registerViewContainer defaults this to `{ id: container.id }`; declare it here so the
	// mnemonic lives on the container. The single view must NOT also register this id.
	openCommandActionDescriptor: {
		id: HORIZON_VIEWLET_ID,
		mnemonicTitle: localize({ key: 'miViewHorizon', comment: ['&& denotes a mnemonic'] }, "&&Horizon"),
		order: 5,
	},
}, ViewContainerLocation.Sidebar);

const horizonViewDescriptor: IViewDescriptor = {
	id: HorizonSidebarView.ID,
	name: HorizonSidebarView.TITLE,
	containerIcon: horizonViewIcon,
	ctorDescriptor: new SyncDescriptor(HorizonSidebarView),
	canToggleVisibility: false,
	canMoveView: true,
	order: 1,
};

Registry.as<IViewsRegistry>(ViewExtensions.ViewsRegistry).registerViews([horizonViewDescriptor], horizonViewContainer);

//#endregion

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

//#region --- Status bar (folder name + analyse state; click toggles Map)

class HorizonStatusBarContribution extends Disposable implements IWorkbenchContribution {

	static readonly ID = 'workbench.contrib.horizonStatusBar';

	private readonly entry: IStatusbarEntryAccessor;

	constructor(
		@IStatusbarService statusbarService: IStatusbarService,
		@IEditorService private readonly editorService: IEditorService,
		@IHorizonAnalysisService private readonly analysisService: IHorizonAnalysisService,
	) {
		super();

		this.entry = this._register(statusbarService.addEntry(
			this.buildEntry(),
			'status.horizonMap',
			StatusbarAlignment.LEFT,
			100 /* high visibility, near problems */
		));

		this._register(editorService.onDidActiveEditorChange(() => this.refresh()));
		this._register(this.analysisService.onDidChangeFolder(() => this.refresh()));
		this._register(this.analysisService.onDidChangeProgress(() => this.refresh()));
	}

	private refresh(): void {
		this.entry.update(this.buildEntry());
	}

	private buildEntry() {
		const mapActive = this.editorService.activeEditor instanceof HorizonMapInput;
		const folder = this.analysisService.getFolder();
		const progress = this.analysisService.progress;
		const folderName = folder?.name;
		const folderTip = folder
			? localize('status.horizonMap.folderTip', 'Folder: {0} ({1})', folder.name, folder.path)
			: localize('status.horizonMap.noFolderTip', 'No analyse folder — choose a folder from the Horizon sidebar');
		const chooseHint = localize('status.horizonMap.chooseHint', 'Use Horizon: Choose Folder… to change the analyse root');

		if (progress.status === 'running') {
			const text = folderName
				? `$(sync~spin) ${localize('status.horizonMap.analysingNamed', 'Horizon: {0}…', folderName)}`
				: `$(sync~spin) ${localize('status.horizonMap.analysing', 'Horizon: analysing…')}`;
			return {
				name: localize('status.horizonMap.name', 'Horizon Map'),
				text,
				ariaLabel: localize('status.horizonMap.ariaAnalysing', 'Horizon is analysing {0}', folderName || 'folder'),
				tooltip: `${localize('status.horizonMap.tipAnalysing', 'Horizon is analysing the workspace')}\n${folderTip}\n${chooseHint}`,
				command: HORIZON_CMD_TOGGLE,
				kind: 'prominent' as const,
			};
		}

		if (progress.status === 'failed' || progress.status === 'error') {
			const text = folderName
				? `$(warning) ${localize('status.horizonMap.failedNamed', 'Horizon: {0} failed', folderName)}`
				: `$(warning) ${localize('status.horizonMap.failed', 'Horizon: analyse failed')}`;
			return {
				name: localize('status.horizonMap.name', 'Horizon Map'),
				text,
				ariaLabel: text,
				tooltip: `${progress.error || localize('status.horizonMap.failed', 'Horizon: analyse failed')}\n${folderTip}\n${chooseHint}`,
				command: HORIZON_CMD_TOGGLE,
			};
		}

		if (progress.status === 'done' && folderName) {
			return {
				name: localize('status.horizonMap.name', 'Horizon Map'),
				text: mapActive
					? `$(map) ${localize('status.horizonMap.activeNamed', 'Horizon Map · {0}', folderName)}`
					: `$(map) ${localize('status.horizonMap.idleNamed', 'Horizon · {0}', folderName)}`,
				ariaLabel: mapActive
					? localize('status.horizonMap.ariaActive', 'Horizon Map is open — click to return to classic editors')
					: localize('status.horizonMap.ariaIdleNamed', 'Open Horizon Map for {0}', folderName),
				tooltip: mapActive
					? `${localize('status.horizonMap.tipActive', 'Click to hide Horizon Map and return to classic editors')}\n${folderTip}\n${chooseHint}`
					: `${localize('status.horizonMap.tipIdle', 'Click to open Horizon Map')}\n${folderTip}\n${chooseHint}`,
				command: HORIZON_CMD_TOGGLE,
				kind: mapActive ? 'prominent' as const : undefined,
			};
		}

		const idleLabel = folderName
			? (mapActive
				? localize('status.horizonMap.activeNamed', 'Horizon Map · {0}', folderName)
				: localize('status.horizonMap.idleNamed', 'Horizon · {0}', folderName))
			: (mapActive
				? localize('status.horizonMap.active', 'Horizon Map')
				: localize('status.horizonMap.idle', 'Horizon'));

		return {
			name: localize('status.horizonMap.name', 'Horizon Map'),
			text: `$(map) ${idleLabel}`,
			ariaLabel: mapActive
				? localize('status.horizonMap.ariaActive', 'Horizon Map is open — click to return to classic editors')
				: localize('status.horizonMap.ariaIdle', 'Open Horizon Map'),
			tooltip: mapActive
				? `${localize('status.horizonMap.tipActive', 'Click to hide Horizon Map and return to classic editors')}\n${folderTip}\n${chooseHint}`
				: `${localize('status.horizonMap.tipIdle', 'Click to open Horizon Map')}\n${folderTip}\n${chooseHint}`,
			command: HORIZON_CMD_TOGGLE,
			kind: mapActive ? 'prominent' as const : undefined,
		};
	}
}

registerWorkbenchContribution2(HorizonStatusBarContribution.ID, HorizonStatusBarContribution, WorkbenchPhase.AfterRestored);

// Ensure context keys are initialized.
void HorizonMapVisibleContext;
void HORIZON_INSPECTION_ACTIVE_CONTEXT;

//#endregion

//#region --- Auto-start analyse (AfterRestored; non-blocking)

/**
 * Once per session after restore: if a workspace (or stored Horizon) folder
 * exists, ensureSidecar + analyse. Soft-fails with a notification. Opens Map
 * on the first successful analyse when no classic editor is critically focused.
 */
class HorizonAutoStartContribution extends Disposable implements IWorkbenchContribution {

	static readonly ID = 'workbench.contrib.horizonAutoStart';

	private static openedMapThisSession = false;

	constructor(
		@IHorizonAnalysisService private readonly analysisService: IHorizonAnalysisService,
		@IWorkspaceContextService private readonly workspaceService: IWorkspaceContextService,
		@INotificationService private readonly notificationService: INotificationService,
		@IEditorService private readonly editorService: IEditorService,
	) {
		super();

		void this.runAutoStart();
	}

	private async runAutoStart(): Promise<void> {
		const folder = this.analysisService.getFolder();
		const hasWorkspaceFolder = this.workspaceService.getWorkspace().folders.length > 0;
		if (!folder && !hasWorkspaceFolder) {
			return;
		}

		try {
			await this.analysisService.ensureSidecar();
			const progress = await this.analysisService.analyse();

			if (progress.status === 'failed' || progress.status === 'error') {
				this.notificationService.notify({
					severity: Severity.Warning,
					message: localize(
						'horizonAutoAnalyseFailed',
						"Horizon auto-analyse: {0}",
						progress.error || 'failed'
					),
				});
				return;
			}

			if (progress.status === 'done' && !HorizonAutoStartContribution.openedMapThisSession) {
				await this.maybeOpenMap();
			}
		} catch (err) {
			const message = err instanceof Error ? err.message : String(err);
			console.warn('[Horizon] auto-analyse failed', err);
			this.notificationService.notify({
				severity: Severity.Warning,
				message: localize('horizonAutoAnalyseFailed', "Horizon auto-analyse: {0}", message),
			});
		}
	}

	/**
	 * Prefer opening Map on first success unless a non-Map editor is already
	 * the active focus target (user is mid-edit in classic).
	 */
	private async maybeOpenMap(): Promise<void> {
		const active = this.editorService.activeEditor;
		const criticallyFocused = !!active && !(active instanceof HorizonMapInput);
		if (criticallyFocused) {
			return;
		}
		HorizonAutoStartContribution.openedMapThisSession = true;
		await this.editorService.openEditor({
			resource: HorizonMapInput.RESOURCE,
			options: {
				pinned: true,
				revealIfOpened: true,
				preserveFocus: true,
			}
		});
	}
}

registerWorkbenchContribution2(HorizonAutoStartContribution.ID, HorizonAutoStartContribution, WorkbenchPhase.AfterRestored);

//#endregion

//#region --- Commands

const horizonCategory = localize2('horizonCategory', 'Horizon');
const mapVisibleWhen = ContextKeyExpr.equals(HORIZON_MAP_VISIBLE_CONTEXT, true);
const mapHiddenWhen = ContextKeyExpr.notEquals(HORIZON_MAP_VISIBLE_CONTEXT, true);

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
				primary: KeyMod.CtrlCmd | KeyMod.Shift | KeyCode.KeyH,
			},
			menu: [
				{
					id: MenuId.EditorTitle,
					group: 'navigation',
					order: 1,
					when: mapHiddenWhen,
				},
				{
					id: MenuId.CompactWindowEditorTitle,
					group: 'navigation',
					order: 1,
					when: mapHiddenWhen,
				},
				{
					id: MenuId.CommandCenter,
					order: 10_000,
				},
				{
					id: MenuId.LayoutControlMenu,
					group: 'z_horizon',
					order: 1,
				},
			],
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
			icon: Codicon.files,
			menu: [
				{
					id: MenuId.EditorTitle,
					group: 'navigation',
					order: 1,
					when: mapVisibleWhen,
				},
				{
					id: MenuId.CompactWindowEditorTitle,
					group: 'navigation',
					order: 1,
					when: mapVisibleWhen,
				},
			],
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
				primary: KeyMod.CtrlCmd | KeyMod.Shift | KeyCode.KeyM,
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
			menu: [
				{
					id: MenuId.EditorTitle,
					group: 'navigation',
					order: 2,
					when: mapVisibleWhen,
				},
				{
					id: MenuId.CompactWindowEditorTitle,
					group: 'navigation',
					order: 2,
					when: mapVisibleWhen,
				},
			],
		});
	}
	async run(accessor: ServicesAccessor): Promise<void> {
		const pane = await openHorizonMap(accessor);
		await pane?.analyseWorkspace();
	}
});

registerAction2(class extends Action2 {
	constructor() {
		super({
			id: HORIZON_CMD_CHOOSE_FOLDER,
			title: localize2('horizonChooseFolder', 'Choose Horizon Folder…'),
			category: horizonCategory,
			f1: true,
			icon: Codicon.folderOpened,
			menu: [
				{
					id: MenuId.EditorTitle,
					group: 'navigation',
					order: 3,
					when: mapVisibleWhen,
				},
				{
					id: MenuId.CompactWindowEditorTitle,
					group: 'navigation',
					order: 3,
					when: mapVisibleWhen,
				},
			],
		});
	}
	async run(accessor: ServicesAccessor): Promise<void> {
		const analysis = accessor.get(IHorizonAnalysisService);
		const notification = accessor.get(INotificationService);
		const folder = await analysis.chooseFolder();
		if (!folder) {
			return;
		}
		const progress = await analysis.analyse(folder.path);
		if (progress.status === 'failed' || progress.status === 'error') {
			notification.notify({
				severity: Severity.Warning,
				message: localize(
					'horizonAnalyseFailed',
					"Horizon analyse: {0}",
					progress.error || 'failed'
				),
			});
			return;
		}
		const editorService = accessor.get(IEditorService);
		if (!(editorService.activeEditor instanceof HorizonMapInput)) {
			await openHorizonMap(accessor);
		}
	}
});

registerAction2(class extends Action2 {
	constructor() {
		super({
			id: HORIZON_CMD_OPEN_SELECTED_FUNCTION,
			title: localize2('horizonOpenSelectedFunction', 'Open Selected Function in Editor'),
			category: horizonCategory,
			f1: true,
		});
	}
	async run(accessor: ServicesAccessor): Promise<void> {
		const inspection = accessor.get(IHorizonInspectionService);
		await inspection.openPendingSelection();
	}
});

registerAction2(class extends Action2 {
	constructor() {
		super({
			id: HORIZON_CMD_TRIGGER_INSPECT_SUGGEST,
			title: localize2('horizonTriggerInspectSuggest', 'Trigger Inspection Completions'),
			category: horizonCategory,
			f1: true,
		});
	}
	async run(accessor: ServicesAccessor): Promise<void> {
		const inspection = accessor.get(IHorizonInspectionService);
		await inspection.triggerSuggest();
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
