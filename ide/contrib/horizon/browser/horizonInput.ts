/*---------------------------------------------------------------------------------------------
 *  Copyright (c) Horizon contributors. All rights reserved.
 *--------------------------------------------------------------------------------------------*/

import { Codicon } from '../../../../base/common/codicons.js';
import { ThemeIcon } from '../../../../base/common/themables.js';
import { URI } from '../../../../base/common/uri.js';
import { localize } from '../../../../nls.js';
import { registerIcon } from '../../../../platform/theme/common/iconRegistry.js';
import { EditorInputCapabilities, IUntypedEditorInput } from '../../../common/editor.js';
import { EditorInput } from '../../../common/editor/editorInput.js';
import { HORIZON_MAP_INPUT_ID, HORIZON_MAP_SCHEME } from '../common/horizon.js';

const horizonMapEditorIcon = registerIcon(
	'horizon-map-editor-label-icon',
	Codicon.map,
	localize('horizonMapEditorLabelIcon', 'Icon of the Horizon Map editor.')
);

/**
 * Singleton EditorInput for the Horizon Map EditorPane.
 */
export class HorizonMapInput extends EditorInput {

	static readonly ID = HORIZON_MAP_INPUT_ID;

	static readonly RESOURCE = URI.from({
		scheme: HORIZON_MAP_SCHEME,
		path: 'map'
	});

	private static _instance: HorizonMapInput | undefined;

	static get instance(): HorizonMapInput {
		if (!HorizonMapInput._instance || HorizonMapInput._instance.isDisposed()) {
			HorizonMapInput._instance = new HorizonMapInput();
		}
		return HorizonMapInput._instance;
	}

	override get typeId(): string {
		return HorizonMapInput.ID;
	}

	override get editorId(): string | undefined {
		return HorizonMapInput.ID;
	}

	override get capabilities(): EditorInputCapabilities {
		return EditorInputCapabilities.Readonly | EditorInputCapabilities.Singleton;
	}

	readonly resource = HorizonMapInput.RESOURCE;

	override getName(): string {
		return localize('horizonMapInputName', "Horizon Map");
	}

	override getIcon(): ThemeIcon {
		return horizonMapEditorIcon;
	}

	override matches(other: EditorInput | IUntypedEditorInput): boolean {
		if (super.matches(other)) {
			return true;
		}
		return other instanceof HorizonMapInput;
	}
}
