/*---------------------------------------------------------------------------------------------
 *  Copyright (c) Horizon contributors. All rights reserved.
 *--------------------------------------------------------------------------------------------*/

/**
 * Desktop Horizon sidecar: spawn/attach `horizon-server`, register singleton
 * (overrides the browser attach-only implementation from common.main).
 */
import { InstantiationType, registerSingleton } from '../../../../platform/instantiation/common/extensions.js';
import { IHorizonSidecarService } from '../common/horizonSidecar.js';
import { ElectronHorizonSidecarService } from './horizonSidecarService.js';

registerSingleton(IHorizonSidecarService, ElectronHorizonSidecarService, InstantiationType.Delayed);
