#!/usr/bin/env node

import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';

const MINIMUM_LIQUIDITY = 1000;
const ACTIVE_POOL_RESERVES = {
  LOGOS: 10_000_000,
  USDC: 1_250_000,
};
const ACTIVE_POOL_LP_SUPPLY = 6_875_000;
const SUPPORTED_FEE_TIERS = new Set([1, 5, 30, 100]);
const ROLE_TO_IDL_NAME = {
  Config: 'config',
  Pool: 'pool',
  'Vault A': 'vault_a',
  'Vault B': 'vault_b',
  'LP definition': 'pool_definition_lp',
  'LP lock holding': 'lp_lock_holding',
  'User holding A': 'user_holding_a',
  'User holding B': 'user_holding_b',
  'User LP holding': 'user_holding_lp',
  'Current tick': 'current_tick_account',
  Clock: 'clock',
};

function stableId(key) {
  return createHash('sha256').update(key).digest('hex');
}

function inferDecimals(totalSupply) {
  const supply = BigInt(totalSupply);
  const digits = supply.toString().length;

  if (supply < 10_000_000n)
    return 0;
  if (digits <= 14)
    return 6;
  if (digits <= 21)
    return 9;
  return 18;
}

function parseHumanAmount(input, decimals) {
  const text = String(input).replaceAll(',', '').trim();
  assert.match(text, /^\d+(\.\d+)?$/, `invalid amount: ${input}`);

  const [whole, fraction = ''] = text.split('.');
  assert.ok(fraction.length <= decimals, `too many decimal places: ${input}`);

  const scale = 10n ** BigInt(decimals);
  const paddedFraction = fraction.padEnd(decimals, '0') || '0';
  return BigInt(whole) * scale + BigInt(paddedFraction);
}

function formatBaseUnits(units, decimals) {
  const value = BigInt(units);
  if (decimals === 0)
    return value.toString();

  const scale = 10n ** BigInt(decimals);
  const whole = value / scale;
  const fraction = (value % scale).toString().padStart(decimals, '0').replace(/0+$/, '');
  return fraction.length > 0 ? `${whole}.${fraction}` : whole.toString();
}

function parsePositiveAmount(value) {
  const parsed = Number(String(value).replaceAll(',', '').trim());
  return Number.isFinite(parsed) && parsed > 0 ? parsed : 0;
}

function unorderedPairKey(symbolA, symbolB) {
  return symbolA < symbolB ? `${symbolA}/${symbolB}` : `${symbolB}/${symbolA}`;
}

function activeRatio(symbolA, symbolB) {
  if (symbolA === 'USDC' && symbolB === 'LOGOS')
    return ACTIVE_POOL_RESERVES.LOGOS / ACTIVE_POOL_RESERVES.USDC;
  if (symbolA === 'LOGOS' && symbolB === 'USDC')
    return ACTIVE_POOL_RESERVES.USDC / ACTIVE_POOL_RESERVES.LOGOS;
  return 1;
}

function defaultInitialPrice(symbolA, symbolB) {
  if (symbolA === 'USDC' && symbolB === 'WETH')
    return 2500;
  if (symbolA === 'WETH' && symbolB === 'USDC')
    return 0.0004;
  return 1;
}

function poolContext(symbolA, symbolB) {
  const pairKey = unorderedPairKey(symbolA, symbolB);
  const poolId = stableId(`devnet:amm-pool:${pairKey}`);

  if (pairKey === 'LOGOS/USDC')
    return { poolId, poolStatus: 'active_pool', instruction: 'add_liquidity', storedFeeBps: 30 };
  if (pairKey === 'USDC/WETH')
    return { poolId, poolStatus: 'missing_pool', instruction: 'new_definition', storedFeeBps: 0 };
  return { poolId, poolStatus: 'unavailable_pool', instruction: '', storedFeeBps: 0 };
}

function accountChange(role, id, action) {
  return { role, id, action };
}

function accountChanges(request, context) {
  const { tokenA, tokenB } = request;
  const { poolId } = context;
  const missingPool = context.poolStatus === 'missing_pool';
  const changes = [
    accountChange('Config', stableId('devnet:amm-config'), 'Read'),
    accountChange('Pool', poolId, missingPool ? 'Create' : 'Update'),
    accountChange('Vault A', stableId(`devnet:vault:${poolId}:${tokenA}`), missingPool ? 'Update or create' : 'Update'),
    accountChange('Vault B', stableId(`devnet:vault:${poolId}:${tokenB}`), missingPool ? 'Update or create' : 'Update'),
    accountChange('LP definition', stableId(`devnet:lp-definition:${poolId}`), missingPool ? 'Create' : 'Update'),
  ];

  if (missingPool)
    changes.push(accountChange('LP lock holding', stableId(`devnet:lp-lock:${poolId}`), 'Create'));

  changes.push(
    accountChange('User holding A', stableId(`devnet:user-holding:${tokenA}`), 'Update'),
    accountChange('User holding B', stableId(`devnet:user-holding:${tokenB}`), 'Update'),
    accountChange('User LP holding', stableId(`devnet:user-lp:${poolId}`), 'Update or create'),
    accountChange('Current tick', stableId(`devnet:current-tick:${poolId}`), missingPool ? 'Create' : 'Update'),
    accountChange('Clock', stableId('devnet:clock:canonical'), 'Read'),
  );

  return changes;
}

function quoteNewPosition(request) {
  assert.ok(SUPPORTED_FEE_TIERS.has(request.feeBps), 'unsupported fee tier');

  const context = poolContext(request.tokenA, request.tokenB);
  const slippageBps = Math.max(1, Math.min(5000, request.slippageBps));

  if (context.poolStatus === 'active_pool') {
    assert.equal(request.feeBps, context.storedFeeBps, 'active pool fee tier mismatch');

    const inputA = parsePositiveAmount(request.amountA);
    const inputB = parsePositiveAmount(request.amountB);
    const editA = request.editedSide !== 'B';
    const ratio = activeRatio(request.tokenA, request.tokenB);
    const amountA = editA ? inputA : inputB / ratio;
    const amountB = editA ? inputA * ratio : inputB;
    const reserveA = ACTIVE_POOL_RESERVES[request.tokenA];
    const reserveB = ACTIVE_POOL_RESERVES[request.tokenB];
    const expectedLp = Math.floor(Math.min(
      ACTIVE_POOL_LP_SUPPLY * amountA / reserveA,
      ACTIVE_POOL_LP_SUPPLY * amountB / reserveB,
    ));

    return {
      accountChanges: accountChanges(request, context),
      deposit: { maxA: amountA, maxB: amountB, actualA: amountA, actualB: amountB },
      instruction: context.instruction,
      lp: {
        expected: expectedLp,
        locked: 0,
        minimum: Math.floor(expectedLp * (10000 - slippageBps) / 10000),
      },
      poolStatus: context.poolStatus,
    };
  }

  if (context.poolStatus === 'missing_pool') {
    const parsedPrice = parsePositiveAmount(request.initialPrice);
    const price = parsedPrice > 0 ? parsedPrice : defaultInitialPrice(request.tokenA, request.tokenB);
    const baseA = price >= 1 ? price : 1;
    const baseB = price >= 1 ? 1 : 1 / price;
    const minimumScale = Math.floor(MINIMUM_LIQUIDITY / Math.sqrt(baseA * baseB)) + 1;
    const scale = minimumScale * Math.max(1, request.depositScale);
    const amountA = baseA * scale;
    const amountB = baseB * scale;
    const initialLp = Math.floor(Math.sqrt(amountA * amountB));
    const userLp = Math.max(0, initialLp - MINIMUM_LIQUIDITY);

    return {
      accountChanges: accountChanges(request, context),
      deposit: { maxA: amountA, maxB: amountB, actualA: amountA, actualB: amountB },
      instruction: context.instruction,
      lp: {
        expected: userLp,
        locked: MINIMUM_LIQUIDITY,
        minimum: Math.floor(userLp * (10000 - slippageBps) / 10000),
      },
      poolStatus: context.poolStatus,
    };
  }

  throw new Error(`unsupported pool: ${request.tokenA}/${request.tokenB}`);
}

function idlAccounts(instructionName) {
  const idl = JSON.parse(readFileSync(new URL('../../../artifacts/amm-idl.json', import.meta.url), 'utf8'));
  const instruction = idl.instructions.find((item) => item.name === instructionName);
  assert.ok(instruction, `missing IDL instruction: ${instructionName}`);
  return instruction.accounts.map((account) => account.name);
}

function assertAccountOrderMatchesIdl(quote, instructionName) {
  const actual = quote.accountChanges.map((change) => {
    assert.ok(ROLE_TO_IDL_NAME[change.role], `unmapped account role: ${change.role}`);
    return ROLE_TO_IDL_NAME[change.role];
  });
  assert.deepEqual(actual, idlAccounts(instructionName));
}

function testDecimals() {
  assert.equal(inferDecimals(9_999_999n), 0);
  assert.equal(inferDecimals(1_000_000_000_000n), 6);
  assert.equal(inferDecimals(1_000_000_000_000_000_000n), 9);
  assert.equal(inferDecimals(1_000_000_000_000_000_000_000_000n), 18);
  assert.ok([9_999_999n, 1_000_000_000_000n, 1_000_000_000_000_000_000n]
    .every((supply) => inferDecimals(supply) !== 8));

  for (const [amount, decimals] of [
    ['12345', 0],
    ['12.3456', 6],
    ['0.000000001', 9],
    ['1.000000000000000001', 18],
  ]) {
    assert.equal(formatBaseUnits(parseHumanAmount(amount, decimals), decimals), amount);
  }

  assert.throws(() => parseHumanAmount('1.1', 0), /too many decimal places/);
  assert.throws(() => parseHumanAmount('1.0000000001', 9), /too many decimal places/);
}

function testMissingPool() {
  const quote1x = quoteNewPosition({
    amountA: '',
    amountB: '',
    depositScale: 1,
    editedSide: 'A',
    feeBps: 30,
    initialPrice: '2500',
    slippageBps: 50,
    tokenA: 'USDC',
    tokenB: 'WETH',
  });
  assert.equal(quote1x.poolStatus, 'missing_pool');
  assert.equal(quote1x.instruction, 'new_definition');
  assert.equal(quote1x.deposit.maxA, 52_500);
  assert.equal(quote1x.deposit.maxB, 21);
  assert.equal(quote1x.lp.expected, 50);
  assert.equal(quote1x.lp.locked, 1000);
  assert.equal(quote1x.lp.minimum, 49);
  assertAccountOrderMatchesIdl(quote1x, 'new_definition');

  const quote2x = quoteNewPosition({
    amountA: '',
    amountB: '',
    depositScale: 2,
    editedSide: 'A',
    feeBps: 30,
    initialPrice: '2500',
    slippageBps: 50,
    tokenA: 'USDC',
    tokenB: 'WETH',
  });
  assert.equal(quote2x.deposit.maxA / quote2x.deposit.maxB, 2500);
  assert.equal(quote2x.deposit.maxA, 105_000);
  assert.equal(quote2x.deposit.maxB, 42);
  assert.equal(quote2x.lp.expected, 1100);
  assert.equal(quote2x.lp.minimum, 1094);
}

function testActivePool() {
  const quoteByA = quoteNewPosition({
    amountA: '100',
    amountB: '',
    depositScale: 1,
    editedSide: 'A',
    feeBps: 30,
    initialPrice: '',
    slippageBps: 50,
    tokenA: 'USDC',
    tokenB: 'LOGOS',
  });
  assert.equal(quoteByA.poolStatus, 'active_pool');
  assert.equal(quoteByA.instruction, 'add_liquidity');
  assert.equal(quoteByA.deposit.maxA, 100);
  assert.equal(quoteByA.deposit.maxB, 800);
  assert.equal(quoteByA.lp.expected, 550);
  assert.equal(quoteByA.lp.minimum, 547);
  assertAccountOrderMatchesIdl(quoteByA, 'add_liquidity');

  const quoteByB = quoteNewPosition({
    amountA: '',
    amountB: '100',
    depositScale: 1,
    editedSide: 'B',
    feeBps: 30,
    initialPrice: '',
    slippageBps: 50,
    tokenA: 'LOGOS',
    tokenB: 'USDC',
  });
  assert.equal(quoteByB.deposit.maxA, 800);
  assert.equal(quoteByB.deposit.maxB, 100);
  assert.equal(quoteByB.lp.expected, 550);
}

testDecimals();
testMissingPool();
testActivePool();

console.log('New Position validation harness passed');
