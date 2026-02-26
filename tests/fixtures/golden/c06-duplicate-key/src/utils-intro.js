// INTRO version: Vulnerable - duplicate wsUrl key

export const HUD = {
  utils: {
    // First wsUrl definition - handles HTTPS correctly
    wsUrl: (loc) => {
      const proto = loc.protocol === 'https:' ? 'wss:' : 'ws:';
      return `${proto}//${loc.host}/ws`;
    },
    
    otherHelper: () => 'helper',
    
    // BUG: Second wsUrl definition overwrites first!
    wsUrl: (loc) => {
      // Missing wss: case - always falls through to ws:
      if (loc.protocol === 'http:') {
        return `ws://${loc.host}/ws`;
      }
      return `ws://${loc.host}/ws`; // Should be wss: for https:
    },
  }
};

export default HUD;
