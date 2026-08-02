/*---------------------------------------------------------------------------------------------
 *  Copyright (c) Horizon contributors. All rights reserved.
 *--------------------------------------------------------------------------------------------*/

import { IContextMenuService } from '../../../../platform/contextview/browser/contextView.js';
import { IKeybindingService } from '../../../../platform/keybinding/common/keybinding.js';
import { IInstantiationService } from '../../../../platform/instantiation/common/instantiation.js';
import { IConfigurationService } from '../../../../platform/configuration/common/configuration.js';
import { IContextKeyService } from '../../../../platform/contextkey/common/contextkey.js';
import { IViewDescriptorService } from '../../../common/views.js';
import { IOpenerService } from '../../../../platform/opener/common/opener.js';
import { IThemeService } from '../../../../platform/theme/common/themeService.js';
import { IHoverService } from '../../../../platform/hover/browser/hover.js';
import { ICommandService } from '../../../../platform/commands/common/commands.js';
import { ViewPane } from '../../../browser/parts/views/viewPane.js';
import { IViewletViewOptions } from '../../../browser/parts/views/viewsViewlet.js';
import { ILocalizedString } from '../../../../platform/action/common/action.js';
import { localize, localize2 } from '../../../../nls.js';
import {
	HORIZON_CMD_ANALYSE,
	HORIZON_CMD_CHOOSE_FOLDER,
	HORIZON_CMD_TOGGLE,
} from '../common/horizon.js';
import { HorizonFolderInfo, IHorizonAnalysisService } from './horizonAnalysis.js';

import './media/horizonSidebar.css';

/**
 * Activity-bar Horizon pane: Map / Analyse / Choose folder without Command Palette.
 */
export class HorizonSidebarView extends ViewPane {

	static readonly ID = 'workbench.view.horizon';
	static readonly TITLE: ILocalizedString = localize2('horizonSidebar', 'Horizon');

	private _folderLabel: HTMLElement | undefined;
	private _statusLabel: HTMLElement | undefined;

	constructor(
		options: IViewletViewOptions,
		@IKeybindingService keybindingService: IKeybindingService,
		@IContextMenuService contextMenuService: IContextMenuService,
		@IConfigurationService configurationService: IConfigurationService,
		@IContextKeyService contextKeyService: IContextKeyService,
		@IViewDescriptorService viewDescriptorService: IViewDescriptorService,
		@IInstantiationService instantiationService: IInstantiationService,
		@IOpenerService openerService: IOpenerService,
		@IThemeService themeService: IThemeService,
		@IHoverService hoverService: IHoverService,
		@ICommandService private readonly commandService: ICommandService,
		@IHorizonAnalysisService private readonly analysisService: IHorizonAnalysisService,
	) {
		super(options, keybindingService, contextMenuService, configurationService, contextKeyService, viewDescriptorService, instantiationService, openerService, themeService, hoverService);
	}

	protected override renderBody(container: HTMLElement): void {
		super.renderBody(container);
		container.classList.add('horizon-sidebar');

		const intro = document.createElement('p');
		intro.className = 'horizon-sidebar-intro';
		intro.textContent = localize(
			'horizonSidebarIntro',
			'Open the free-function Map, pick an analyse folder, or re-run analysis. JSON upload is advanced-only.'
		);
		container.appendChild(intro);

		const folderBlock = document.createElement('div');
		folderBlock.className = 'horizon-sidebar-folder';
		const folderCaption = document.createElement('div');
		folderCaption.className = 'horizon-sidebar-folder-caption';
		folderCaption.textContent = localize('horizonSidebarFolderCaption', 'Analyse folder');
		folderBlock.appendChild(folderCaption);
		this._folderLabel = document.createElement('div');
		this._folderLabel.className = 'horizon-sidebar-folder-name';
		folderBlock.appendChild(this._folderLabel);
		container.appendChild(folderBlock);

		this._statusLabel = document.createElement('div');
		this._statusLabel.className = 'horizon-sidebar-status';
		container.appendChild(this._statusLabel);

		container.appendChild(this.makeButton(
			localize('horizonSidebarOpenMap', 'Open / Toggle Map'),
			localize('horizonSidebarOpenMapHint', 'Show or hide the visual map'),
			() => this.commandService.executeCommand(HORIZON_CMD_TOGGLE)
		));

		container.appendChild(this.makeButton(
			localize('horizonSidebarChooseFolder', 'Choose folder…'),
			localize('horizonSidebarChooseFolderHint', 'Pick a workspace folder or browse on disk'),
			() => this.commandService.executeCommand(HORIZON_CMD_CHOOSE_FOLDER),
			true
		));

		container.appendChild(this.makeButton(
			localize('horizonSidebarAnalyse', 'Analyse Workspace'),
			localize('horizonSidebarAnalyseHint', 'Run Horizon on the chosen folder and open the map'),
			() => this.commandService.executeCommand(HORIZON_CMD_ANALYSE)
		));

		this.refreshFolder(this.analysisService.getFolder());
		this.refreshStatus();

		this._register(this.analysisService.onDidChangeFolder(folder => this.refreshFolder(folder)));
		this._register(this.analysisService.onDidChangeProgress(() => this.refreshStatus()));
	}

	private refreshFolder(folder: HorizonFolderInfo | undefined): void {
		if (!this._folderLabel) {
			return;
		}
		if (!folder) {
			this._folderLabel.textContent = localize('horizonSidebarNoFolder', 'No folder open');
			this._folderLabel.title = '';
			return;
		}
		this._folderLabel.textContent = folder.name;
		this._folderLabel.title = folder.path;
	}

	private refreshStatus(): void {
		if (!this._statusLabel) {
			return;
		}
		const progress = this.analysisService.progress;
		switch (progress.status) {
			case 'running':
				this._statusLabel.textContent = localize('horizonSidebarAnalysing', 'Analysing…');
				this._statusLabel.hidden = false;
				break;
			case 'failed':
			case 'error':
				this._statusLabel.textContent = progress.error
					? localize('horizonSidebarFailed', 'Analyse failed')
					: localize('horizonSidebarFailed', 'Analyse failed');
				this._statusLabel.title = progress.error || '';
				this._statusLabel.hidden = false;
				break;
			case 'done':
				this._statusLabel.textContent = localize('horizonSidebarReady', 'Map ready');
				this._statusLabel.hidden = false;
				break;
			default:
				this._statusLabel.textContent = '';
				this._statusLabel.title = '';
				this._statusLabel.hidden = true;
				break;
		}
	}

	private makeButton(label: string, title: string, onClick: () => void, secondary = false): HTMLButtonElement {
		const btn = document.createElement('button');
		btn.type = 'button';
		btn.className = secondary ? 'horizon-sidebar-btn secondary' : 'horizon-sidebar-btn';
		btn.textContent = label;
		btn.title = title;
		btn.addEventListener('click', () => onClick());
		return btn;
	}
}
