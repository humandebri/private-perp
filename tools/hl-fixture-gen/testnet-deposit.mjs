// Explicit live-test helper: one owner-wallet transfer to the prepared reserve.
import { readFileSync, writeFileSync, existsSync } from 'node:fs';
import { privateKeyToAccount } from 'viem/accounts';
import { usdSend } from '@nktkas/hyperliquid/api/exchange';
if (process.env.TESTNET_ACCEPTANCE !== '1') throw new Error('TESTNET_ACCEPTANCE=1 required');
const publicState = JSON.parse(readFileSync('../../.icp-home/hl-testnet/public.json','utf8'));
const record = '../../.icp-home/hl-testnet/deposit-'+process.env.TESTNET_APP_ID+'.json';
if (!process.env.TESTNET_APP_ID) throw new Error('TESTNET_APP_ID required');
if (existsSync(record)) throw new Error('Deposit already recorded; reconcile instead of sending again');
const wallet = privateKeyToAccount('0x'+process.env.PRIVATE_PERP_TESTNET_EOA_KEY.replace(/^0x/,''));
if (wallet.address.toLowerCase() !== publicState.ownerAddress.toLowerCase()) throw new Error('Owner mismatch');
if (publicState.hlNetwork !== 'testnet') throw new Error('Testnet required');
const destination = publicState.reserveAddress;
if (!/^0x[0-9a-fA-F]{40}$/.test(destination)) throw new Error('Invalid reserve');
const response = await fetch('https://api.hyperliquid-testnet.xyz/info',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({type:'clearinghouseState',user:wallet.address})});
const balance = await response.json();
if (!response.ok || Number(balance.withdrawable)<9) throw new Error('Owner needs 9 available test USDC');
let sent = false;
const transport = {isTestnet:true,async request(endpoint,payload) {
  if (sent || endpoint!=='exchange' || payload.action.hyperliquidChain!=='Testnet' || payload.action.type!=='usdSend' || payload.action.amount!=='9' || payload.action.destination.toLowerCase()!==destination.toLowerCase()) throw new Error('Unexpected deposit');
  sent=true;
  writeFileSync(record,JSON.stringify({phase:'prepared',owner:wallet.address,destination,request:payload},null,2)+'\n',{flag:'wx',mode:0o600});
  const response=await fetch('https://api.hyperliquid-testnet.xyz/exchange',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(payload),signal:AbortSignal.timeout(15000)});
  const result=await response.json();
  writeFileSync(record,JSON.stringify({phase:'response',owner:wallet.address,destination,request:payload,httpStatus:response.status,response:result},null,2)+'\n',{mode:0o600});
  console.log(JSON.stringify({destination,amount:'9',response:result}));
  return result;
}};
await usdSend({transport,wallet},{destination,amount:'9'});
