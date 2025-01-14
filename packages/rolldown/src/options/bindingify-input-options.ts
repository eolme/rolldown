import { BindingInputOptions, BindingLogLevel } from '../binding'
import type {
  BindingInjectImportNamed,
  BindingInjectImportNamespace,
  BindingWatchOption,
} from '../binding'
import { bindingifyPlugin } from '../plugin/bindingify-plugin'
import type { NormalizedInputOptions } from './normalized-input-options'
import { arraify, unsupported } from '../utils/misc'
import type { NormalizedOutputOptions } from './normalized-output-options'
import type { LogLevelOption } from '../log/logging'
import {
  bindingifyBuiltInPlugin,
  BuiltinPlugin,
} from '../plugin/builtin-plugin'
import { PluginContextData } from '../plugin/plugin-context-data'
import { normalizedStringOrRegex } from '../utils/normalize-string-or-regex'

export function bindingifyInputOptions(
  options: NormalizedInputOptions,
  outputOptions: NormalizedOutputOptions,
): BindingInputOptions {
  const pluginContextData = new PluginContextData()
  return {
    input: bindingifyInput(options.input),
    plugins: options.plugins.map((plugin) => {
      if ('_parallel' in plugin) {
        return undefined
      }
      if (plugin instanceof BuiltinPlugin) {
        return bindingifyBuiltInPlugin(plugin)
      }
      return bindingifyPlugin(plugin, options, outputOptions, pluginContextData)
    }),
    cwd: options.cwd ?? process.cwd(),
    external: options.external
      ? (function bindingifyExternal() {
          const external = options.external
          if (typeof external === 'function') {
            return (id, importer, isResolved) => {
              if (id.startsWith('\0')) return false
              return external(id, importer, isResolved) ?? false
            }
          }
          const externalArr = arraify(external)
          return (id, _importer, _isResolved) => {
            return externalArr.some((pat) => {
              if (pat instanceof RegExp) {
                return pat.test(id)
              }
              return id === pat
            })
          }
        })()
      : undefined,
    resolve: options.resolve
      ? (function bindingifyResolve() {
          const { alias, extensionAlias, ...rest } = options.resolve

          return {
            alias: alias
              ? Object.entries(alias).map(([name, replacement]) => ({
                  find: name,
                  replacements: [replacement],
                }))
              : undefined,
            extensionAlias: extensionAlias
              ? Object.entries(extensionAlias).map(([name, value]) => ({
                  target: name,
                  replacements: value,
                }))
              : undefined,
            ...rest,
          }
        })()
      : undefined,
    platform: options.platform,
    shimMissingExports: options.shimMissingExports,
    // @ts-ignore TODO The typing should import from binding
    logLevel: bindingifyLogLevel(options.logLevel),
    onLog: (level, log) => {
      options.onLog(level, { code: log.code, message: log.message })
    },
    treeshake: options.treeshake,
    moduleTypes: options.moduleTypes,
    define: options.define ? Object.entries(options.define) : undefined,
    inject: options.inject
      ? Object.entries(options.inject).map(
          ([alias, item]):
            | BindingInjectImportNamed
            | BindingInjectImportNamespace => {
            if (Array.isArray(item)) {
              // import * as fs from 'node:fs'
              // fs: ['node:fs', '*' ],
              if (item[1] === '*') {
                return {
                  tagNamespace: true,
                  alias,
                  from: item[0],
                }
              }

              // import { Promise } from 'es6-promise'
              // Promise: [ 'es6-promise', 'Promise' ],

              // import { Promise as P } from 'es6-promise'
              // P: [ 'es6-promise', 'Promise' ],
              return {
                tagNamed: true,
                alias,
                from: item[0],
                imported: item[1],
              }
            } else {
              // import $ from 'jquery'
              // $: 'jquery',

              // 'Object.assign': path.resolve( 'src/helpers/object-assign.js' ),
              return {
                tagNamed: true,
                imported: 'default',
                alias,
                from: item,
              }
            }
          },
        )
      : undefined,
    experimental: {
      strictExecutionOrder: options.experimental?.strictExecutionOrder,
      disableLiveBindings: options.experimental?.disableLiveBindings,
    },
    profilerNames: options?.profilerNames,
    jsx: bindingifyJsx(options.jsx),
    watch: bindingifyWatch(options.watch),
    dropLabels: options.dropLabels,
  }
}

function bindingifyLogLevel(
  logLevel: LogLevelOption,
): BindingLogLevel | undefined {
  switch (logLevel) {
    case 'silent':
      return BindingLogLevel.Silent
    case 'warn':
      return BindingLogLevel.Warn
    case 'info':
      return BindingLogLevel.Info
    case 'debug':
      return BindingLogLevel.Debug

    default:
      throw new Error(`Unexpected log level: ${logLevel}`)
  }
}

function bindingifyInput(
  input: NormalizedInputOptions['input'],
): BindingInputOptions['input'] {
  if (Array.isArray(input)) {
    return input.map((src) => {
      return {
        import: src,
      }
    })
  } else {
    return Object.entries(input).map((value) => {
      return { name: value[0], import: value[1] }
    })
  }
}

function bindingifyJsx(
  input: NormalizedInputOptions['jsx'],
): BindingInputOptions['jsx'] {
  if (input) {
    const mode = input.mode ?? 'classic'
    return {
      runtime: mode,
      importSource:
        mode === 'classic'
          ? input.importSource
          : mode === 'automatic'
            ? input.jsxImportSource
            : undefined,
      pragma: input.factory,
      pragmaFrag: input.fragment,
      development: input.development,
      refresh: input.refresh,
    }
  }
}

function bindingifyWatch(
  watch: NormalizedInputOptions['watch'],
): BindingInputOptions['watch'] {
  if (watch) {
    let value = {
      skipWrite: watch.skipWrite,
      include: normalizedStringOrRegex(watch.include),
      exclude: normalizedStringOrRegex(watch.exclude),
    } as BindingWatchOption
    if (watch.notify) {
      value.notify = {
        pollInterval: watch.notify.pollInterval,
        compareContents: watch.notify.compareContents,
      }
    }
    if (watch.chokidar) {
      unsupported(
        'The watch chokidar option is deprecated, please use notify options instead of it.',
      )
    }
    return value
  }
}
