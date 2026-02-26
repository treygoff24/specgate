// FIX version: Hardened - single wsUrl definition

export const HUD = {
  utils: {
    // Single wsUrl definition - handles HTTPS correctly
    wsUrl: (loc) => {
      const proto = loc.protocol === 'https:' ? 'wss:' : 'ws:';
      return `${proto}//${loc.host}/ws`;
    },
    
    otherHelper: () => 'helper',
  }
};

export default HUD;
